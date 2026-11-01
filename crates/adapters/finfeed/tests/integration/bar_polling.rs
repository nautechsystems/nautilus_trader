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

//! Completed-bar polling, publication cutoffs, cancellation, and reconnect behavior.

use std::time::Duration;

use nautilus_common::{
    clients::DataClient,
    messages::{DataEvent, data::*},
};
use nautilus_core::{
    UUID4, UnixNanos, string::secret::SecretString, time::get_atomic_clock_realtime,
};
use nautilus_finfeed::{
    common::symbol::to_instrument_id, config::FinFeedDataClientConfig, data::FinFeedDataClient,
    http::client::FinFeedHttpConfig, instruments::FinFeedInstrumentSelection,
};
use nautilus_model::{
    data::{Bar, BarType, Data, QuoteTick},
    identifiers::ClientId,
    types::{Price, Quantity},
};
use nautilus_network::dst::{task, time};
use rstest::rstest;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::{
    dst::{bind_peer, read_request, start_peer},
    polling::{connected_selection, timed_peer},
};

const EXCHANGES: &str = include_str!("../../test_data/exchanges.json");
const MARKET: &str = include_str!("../../test_data/polymarket-market.json");
const PERIODS: &str = include_str!("../../test_data/periods.json");
const BARS: &str = include_str!("../../test_data/client-bars.json");
const MARKET_ID: &str = "TEST-OUTCOME_NO";

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn public_bars_start_at_subscribe_and_restore_without_duplicates() {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let (listener, address) = bind_peer().await;

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        attempt_budget: 12,
        ..Default::default()
    };

    let (ready, mut observed) = tokio::sync::mpsc::unbounded_channel();

    let peer = task::spawn(async move {
        let mut requests = Vec::new();
        let mut fresh = String::new();

        for index in 0..9 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            ready
                .send(request.lines().next().unwrap().to_owned())
                .unwrap();
            requests.push(request);

            let body = match index {
                0 | 5 => EXCHANGES,
                1 | 6 => MARKET,
                2 | 7 => PERIODS,
                3 => {
                    time::sleep(Duration::from_millis(1100)).await;
                    fresh = closed_rows();
                    BARS
                }
                4 | 8 => &fresh,
                _ => unreachable!(),
            };

            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        }

        requests
    });

    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    let bar_type = bar_type();
    client.subscribe_bars(subscribe(bar_type)).unwrap();
    client.subscribe_bars(subscribe(bar_type)).unwrap();
    let first = bar(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert!(receiver.try_recv().is_err());
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));

    for _ in 0..9 {
        time::timeout(Duration::from_secs(5), observed.recv())
            .await
            .unwrap()
            .unwrap();
    }

    time::sleep(Duration::from_millis(50)).await;
    client.unsubscribe_bars(&unsubscribe(bar_type)).unwrap();
    client.disconnect().await.unwrap();
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 9);
    assert_eq!(
        requests[2].lines().next().unwrap(),
        "GET /v1/ohlcv/periods HTTP/1.1"
    );

    for index in [3, 4, 8] {
        assert_eq!(
            requests[index].lines().next().unwrap(),
            format!(
                "GET /v1/ohlcv/POLYMARKET/{MARKET_ID}/latest?period_id=1SEC&limit=101 HTTP/1.1"
            )
        );
    }

    assert_eq!(
        requests[7].lines().next().unwrap(),
        "GET /v1/ohlcv/periods HTTP/1.1"
    );
    assert_eq!(first.bar_type, bar_type);
    assert_eq!(first.open.to_string(), "0.2000");
    assert_eq!(first.high.to_string(), "0.4000");
    assert_eq!(first.low.to_string(), "0.1000");
    assert_eq!(first.close.to_string(), "0.3000");
    assert_eq!(first.volume.to_string(), "12.345678");
    assert_eq!(first.ts_event, first.ts_init);
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case("publish")]
#[case("unsubscribe")]
#[case("disconnect")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bar_unsubscribe_and_disconnect_cancel_delayed_latest(#[case] action: &str) {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let (listener, address) = bind_peer().await;

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        ..Default::default()
    };

    let (ready, mut observed) = tokio::sync::mpsc::unbounded_channel();

    let peer = task::spawn(async move {
        let mut end = 0;

        for (index, body) in [EXCHANGES, MARKET, PERIODS].into_iter().enumerate() {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;

            if index == 2 {
                end = get_atomic_clock_realtime().get_time_ns().as_u64() / 100 * 100;
            }

            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        }

        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        ready.send(()).unwrap();
        time::sleep(Duration::from_millis(200)).await;
        let body = rows_at(end);
        let _ = stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await;
        let _ = stream.flush().await;
        (request, end)
    });

    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    client.subscribe_bars(subscribe(bar_type())).unwrap();
    time::timeout(Duration::from_secs(5), observed.recv())
        .await
        .unwrap()
        .unwrap();

    match action {
        "disconnect" => client.disconnect().await.unwrap(),
        "unsubscribe" => client.unsubscribe_bars(&unsubscribe(bar_type())).unwrap(),
        "publish" => {}
        _ => unreachable!(),
    }

    let (request, end) = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    time::sleep(Duration::from_millis(250)).await;
    client.disconnect().await.unwrap();
    assert_eq!(
        request.lines().next().unwrap(),
        format!("GET /v1/ohlcv/POLYMARKET/{MARKET_ID}/latest?period_id=1SEC&limit=101 HTTP/1.1")
    );

    if action == "publish" {
        let expected = Bar::new_checked(
            bar_type(),
            Price::from("0.2000"),
            Price::from("0.4000"),
            Price::from("0.1000"),
            Price::from("0.3000"),
            Quantity::from("12.345678"),
            UnixNanos::from(end),
            UnixNanos::from(end),
        )
        .unwrap();

        let DataEvent::Data(Data::Bar(actual)) = receiver.try_recv().unwrap() else {
            panic!("expected bar")
        };

        assert_eq!(actual, expected);
    }

    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bar_admission_rejects_unsupported_prices_aggregations_and_unloaded_instruments() {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let (peer, http) = start_peer(vec![(200, "", EXCHANGES), (200, "", MARKET)]).await;
    let (mut client, _) = connected_selection(http, vec![MARKET_ID.into()], 100).await;

    for suffix in [
        "1-MINUTE-BID-EXTERNAL",
        "1-MINUTE-LAST-INTERNAL",
        "1-TICK-LAST-EXTERNAL",
    ] {
        let bar_type = BarType::from(
            format!(
                "{}-{suffix}",
                to_instrument_id("POLYMARKET", MARKET_ID).unwrap()
            )
            .as_str(),
        );
        assert!(client.subscribe_bars(subscribe(bar_type)).is_err());
    }

    let other = BarType::from("other_NO.FINFEED_POLYMARKET-1-SECOND-LAST-EXTERNAL");
    assert!(
        client
            .subscribe_bars(subscribe(other))
            .unwrap_err()
            .to_string()
            .contains("not a loaded")
    );
    client.disconnect().await.unwrap();
    assert_eq!(
        client
            .subscribe_bars(subscribe(bar_type()))
            .unwrap_err()
            .to_string(),
        "FinFeed client is disconnected"
    );
    assert_eq!(peer.await.unwrap().len(), 2);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn response_crossing_period_end_uses_request_cutoff_and_final_later_values() {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let (listener, address) = bind_peer().await;

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        ..Default::default()
    };

    let (expected_sender, mut expected_receiver) = tokio::sync::mpsc::unbounded_channel();

    let peer = task::spawn(async move {
        let mut requests = Vec::new();

        for body in [EXCHANGES, MARKET, PERIODS] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut stream).await);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        }

        let (mut stream, _) = listener.accept().await.unwrap();
        requests.push(read_request(&mut stream).await);
        let end = (get_atomic_clock_realtime().get_time_ns().as_u64() + 200_000_000) / 100 * 100;
        let source = rows_at(end);
        time::sleep(Duration::from_millis(400)).await;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{source}",
                    source.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let (mut stream, _) = listener.accept().await.unwrap();
        requests.push(read_request(&mut stream).await);
        let mut final_rows: Value = serde_json::from_str(&source).unwrap();
        final_rows[0]["price_close"] = Value::String("0.35".into());
        let body = final_rows.to_string();
        expected_sender.send(end).unwrap();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.flush().await.unwrap();
        requests
    });

    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    client.subscribe_bars(subscribe(bar_type())).unwrap();
    let actual = bar(&mut receiver).await;
    let end = expected_receiver.recv().await.unwrap();
    client.unsubscribe_bars(&unsubscribe(bar_type())).unwrap();
    client.disconnect().await.unwrap();
    let expected = Bar::new_checked(
        bar_type(),
        Price::from("0.2000"),
        Price::from("0.4000"),
        Price::from("0.1000"),
        Price::from("0.3500"),
        Quantity::from("12.345678"),
        UnixNanos::from(end),
        UnixNanos::from(end),
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(peer.await.unwrap().len(), 5);
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn canceling_catalog_resolution_keeps_the_quote_scope_active() {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let (listener, address) = bind_peer().await;

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        ..Default::default()
    };

    let (ready, mut observed) = tokio::sync::mpsc::unbounded_channel();

    let peer = task::spawn(async move {
        let mut requests = Vec::new();

        for body in [EXCHANGES, MARKET] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut stream).await);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        }

        let (mut stream, _) = listener.accept().await.unwrap();
        requests.push(read_request(&mut stream).await);
        ready.send(()).unwrap();
        time::sleep(Duration::from_millis(200)).await;
        let _ = stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PERIODS}",
                    PERIODS.len()
                )
                .as_bytes(),
            )
            .await;
        let _ = stream.flush().await;
        let (mut stream, _) = listener.accept().await.unwrap();
        requests.push(read_request(&mut stream).await);
        let body = include_str!("../../test_data/poll-older.json");
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.flush().await.unwrap();
        requests
    });

    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    client.subscribe_bars(subscribe(bar_type())).unwrap();
    time::timeout(Duration::from_secs(5), observed.recv())
        .await
        .unwrap()
        .unwrap();
    client
        .subscribe_quotes(SubscribeQuotes::new(
            bar_type().instrument_id(),
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
            None,
        ))
        .unwrap();
    client.unsubscribe_bars(&unsubscribe(bar_type())).unwrap();
    let event = time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .unwrap()
        .unwrap();

    let DataEvent::Data(Data::Quote(actual)) = event else {
        panic!("expected quote")
    };

    client.disconnect().await.unwrap();
    let expected = QuoteTick::new_checked(
        bar_type().instrument_id(),
        Price::from("0.2000"),
        Price::from("0.3000"),
        Quantity::from("5.000005"),
        Quantity::from("6.000006"),
        UnixNanos::from(1_791_676_800_000_000_000),
        UnixNanos::from(1_791_676_801_000_000_000),
    )
    .unwrap();
    assert_eq!(actual, expected);
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[2].lines().next().unwrap(),
        "GET /v1/ohlcv/periods HTTP/1.1"
    );
    assert_eq!(
        requests[3].lines().next().unwrap(),
        format!("GET /v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=100 HTTP/1.1")
    );
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(404)]
#[case(429)]
#[case(500)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn failed_bar_poll_keeps_the_schedule_and_cached_period(#[case] status: u16) {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, PERIODS, Duration::ZERO),
        (status, "[]", Duration::ZERO),
        (200, "[]", Duration::ZERO),
    ])
    .await;
    http.retry.max_retries = 0;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    client.subscribe_bars(subscribe(bar_type())).unwrap();

    for _ in 0..5 {
        time::timeout(Duration::from_secs(5), observed.recv())
            .await
            .unwrap()
            .unwrap();
    }

    client.unsubscribe_bars(&unsubscribe(bar_type())).unwrap();
    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[3].lines().next(), requests[4].lines().next());
    assert_eq!(
        requests[3].lines().next().unwrap(),
        format!("GET /v1/ohlcv/POLYMARKET/{MARKET_ID}/latest?period_id=1SEC&limit=101 HTTP/1.1")
    );
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(200, PERIODS)]
#[case(401, "[]")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unresolved_bar_period_suspends_without_repeating_catalog_requests(
    #[case] status: u16,
    #[case] body: &'static str,
) {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (status, body, Duration::ZERO),
    ])
    .await;
    http.retry.max_retries = 0;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    let unlisted = BarType::from(
        format!(
            "{}-2-WEEK-LAST-EXTERNAL",
            to_instrument_id("POLYMARKET", MARKET_ID).unwrap()
        )
        .as_str(),
    );
    client.subscribe_bars(subscribe(unlisted)).unwrap();

    for _ in 0..3 {
        time::timeout(Duration::from_secs(5), observed.recv())
            .await
            .unwrap()
            .unwrap();
    }

    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    client.disconnect().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2].lines().next().unwrap(),
        "GET /v1/ohlcv/periods HTTP/1.1"
    );
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(1)]
#[case(1_000)]
#[case(1_001)]
#[case(10_000)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bar_poll_limit_leaves_a_slot_for_the_closed_period(#[case] limit: u32) {
    #[cfg(all(feature = "simulation", madsim))]
    reset_clock();

    let logs = super::polling::capture_diagnostics();
    let first_message = logs.lock().unwrap().len();
    let (listener, address) = bind_peer().await;

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        ..Default::default()
    };

    let peer = task::spawn(async move {
        let mut requests = Vec::new();
        let mut end = 0;

        for body in [EXCHANGES, MARKET, PERIODS] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut stream).await);

            if body == PERIODS {
                time::sleep(Duration::from_millis(1100)).await;
                end = get_atomic_clock_realtime().get_time_ns().as_u64() / 100 * 100;
            }

            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        }

        let (mut stream, _) = listener.accept().await.unwrap();
        time::sleep(Duration::from_millis(200)).await;
        requests.push(read_request(&mut stream).await);
        let now = get_atomic_clock_realtime().get_time_ns().as_u64() / 100 * 100;
        let running: Value = serde_json::from_str(&rows_at(now + 1_000_000_000)).unwrap();
        let closed: Value = serde_json::from_str(&rows_at(end)).unwrap();
        let body = serde_json::to_string(&vec![&running[0], &closed[0]]).unwrap();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.flush().await.unwrap();
        (requests, end)
    });

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http,
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
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    client.subscribe_bars(subscribe(bar_type())).unwrap();
    let actual = bar(&mut receiver).await;
    client.unsubscribe_bars(&unsubscribe(bar_type())).unwrap();
    client.disconnect().await.unwrap();
    let (requests, end) = peer.await.unwrap();
    let expected = Bar::new_checked(
        bar_type(),
        Price::from("0.2000"),
        Price::from("0.4000"),
        Price::from("0.1000"),
        Price::from("0.3000"),
        Quantity::from("12.345678"),
        UnixNanos::from(end),
        UnixNanos::from(end),
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[3].lines().next().unwrap(),
        format!(
            "GET /v1/ohlcv/POLYMARKET/{MARKET_ID}/latest?period_id=1SEC&limit={} HTTP/1.1",
            limit + 1
        )
    );
    assert!(receiver.try_recv().is_err());
    let messages = logs.lock().unwrap();
    let diagnostic = format!(
        "FinFeed OHLCV received for {}, records=2, limit={}, saturated={}",
        bar_type(),
        limit + 1,
        limit == 1
    );
    assert!(messages[first_message..].contains(&(log::Level::Debug, diagnostic)));
}

#[cfg(all(feature = "simulation", madsim))]
fn reset_clock() {
    // Madsim repeats fresh runtimes in one process, retaining the global clock
    let clock = get_atomic_clock_realtime();
    clock.make_static();
    clock.make_realtime();
}

async fn bar(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Bar {
    let event = time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .unwrap()
        .unwrap();

    let DataEvent::Data(Data::Bar(bar)) = event else {
        panic!("expected bar")
    };

    bar
}

fn closed_rows() -> String {
    let end = get_atomic_clock_realtime().get_time_ns().as_u64() / 100 * 100;
    rows_at(end)
}

fn rows_at(end: u64) -> String {
    let start = end - 1_000_000_000;
    let mut rows: Value = serde_json::from_str(BARS).unwrap();
    rows.as_array_mut().unwrap().truncate(1);
    let row = &mut rows[0];
    for (field, ts) in [
        ("time_period_start", start),
        ("time_period_end", end),
        ("time_open", start + 100),
        ("time_close", end - 100),
    ] {
        row[field] = Value::String(UnixNanos::from(ts).to_datetime_utc().to_string());
    }

    rows.to_string()
}

fn bar_type() -> BarType {
    BarType::from(
        format!(
            "{}-1-SECOND-LAST-EXTERNAL",
            to_instrument_id("POLYMARKET", MARKET_ID).unwrap()
        )
        .as_str(),
    )
}

fn subscribe(bar_type: BarType) -> SubscribeBars {
    SubscribeBars::new(
        bar_type,
        Some(ClientId::from("FINFEED")),
        None,
        UUID4::new(),
        UnixNanos::from(0),
        None,
        None,
    )
}

fn unsubscribe(bar_type: BarType) -> UnsubscribeBars {
    UnsubscribeBars::new(
        bar_type,
        Some(ClientId::from("FINFEED")),
        None,
        UUID4::new(),
        UnixNanos::from(0),
        None,
        None,
    )
}
