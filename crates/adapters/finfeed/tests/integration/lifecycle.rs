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

//! Market status refreshes, subscription cancellation, and reconnect behavior.

use std::time::Duration;

use nautilus_common::{
    clients::DataClient,
    messages::{DataEvent, DataResponse, data::*},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_finfeed::{
    common::symbol::to_instrument_id, data::FinFeedDataClient, http::client::FinFeedHttpConfig,
};
use nautilus_model::{
    data::{BarType, Data},
    enums::BookType,
    identifiers::InstrumentId,
    instruments::Instrument,
};
use nautilus_network::dst::{net::TcpStream, task, time};
use rstest::rstest;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::{
    data::response,
    dst::{bind_peer, read_request},
    polling::connected_selection,
};

const EXCHANGES: &str = include_str!("../../test_data/exchanges.json");
const MARKET: &str = include_str!("../../test_data/polymarket-market.json");
const ACTIVITY: &str = include_str!("../../test_data/poll-activity.json");
const BOOK: &str = include_str!("../../test_data/poll-book.json");
const PERIODS: &str = include_str!("../../test_data/periods.json");
const EMPTY: &str = include_str!("../../test_data/empty.json");
const MARKET_ID: &str = "TEST-OUTCOME_NO";

#[rstest]
#[case("Closed", "activity", false)]
#[case("Resolved", "activity", true)]
#[case("Closed", "book", true)]
#[case("Resolved", "book", false)]
#[case("Closed", "periods", false)]
#[case("Resolved", "periods", true)]
#[case("Closed", "bars", true)]
#[case("Resolved", "bars", false)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn terminal_refresh_cancels_all_feeds_and_retains_history_across_reconnect(
    #[case] status: &str,
    #[case] inflight: &str,
    #[case] list: bool,
) {
    let terminal = metadata(status);
    let mut replies = vec![EXCHANGES.to_owned(), MARKET.to_owned()];
    if inflight == "bars" {
        replies.push(PERIODS.to_owned());
    }

    let blocked = replies.len();
    replies.push(
        match inflight {
            "activity" => ACTIVITY,
            "book" => BOOK,
            "periods" => PERIODS,
            "bars" => include_str!("../../test_data/client-bars.json"),
            _ => unreachable!(),
        }
        .to_owned(),
    );

    replies.extend([
        terminal.clone(),
        terminal.clone(),
        EMPTY.to_owned(),
        EXCHANGES.to_owned(),
        terminal,
    ]);
    let (peer, http, mut observed, release) = refresh_peer(replies, blocked).await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, inflight).unwrap();
    if inflight == "activity" {
        subscribe(&mut client, "trade").unwrap();
    } else if inflight == "book" {
        subscribe(&mut client, "depth").unwrap();
    }

    for index in 0..=blocked {
        assert_eq!(observed.recv().await.unwrap(), index);
    }

    for feed in ["activity", "trade", "book", "depth", "bars"] {
        subscribe(&mut client, feed).unwrap();
    }

    refresh(&client, list);
    assert_definition(response(&mut receiver).await, status);
    release.send(()).unwrap();

    for feed in ["activity", "trade", "book", "depth", "bars"] {
        assert_eq!(
            subscribe(&mut client, feed).unwrap_err().to_string(),
            format!("FinFeed instrument {} is inactive or resolved", id())
        );
    }

    refresh(&client, list);
    assert_definition(response(&mut receiver).await, status);
    client
        .request_quotes(RequestQuotes::new(
            id(),
            Some("2026-10-11T00:00:00Z".parse().unwrap()),
            Some("2026-10-11T00:01:00Z".parse().unwrap()),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
        ))
        .unwrap();

    let DataResponse::Quotes(history) = response(&mut receiver).await else {
        panic!("expected history")
    };

    assert_eq!(history.instrument_ids, vec![id()]);
    assert!(history.data.is_empty());
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
        panic!("expected retained definition")
    };

    assert_eq!(instrument.id(), id());
    assert_eq!(instrument.info().unwrap().get_str("status"), Some(status));
    assert!(subscribe(&mut client, "activity").is_err());
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    client.disconnect().await.unwrap();
    assert_eq!(requests.len(), blocked + 6);
    assert!(receiver.try_recv().is_err());
    assert!(requests[blocked].contains(match inflight {
        "activity" => "/latest?limit=100",
        "book" => "/current",
        "periods" => "/ohlcv/periods",
        "bars" => "/latest?period_id=1SEC&limit=101",
        _ => unreachable!(),
    }));
}

#[rstest]
#[case("Open", "Open")]
#[case("Suspended", "Suspended")]
#[case("Unknown", "Unknown")]
#[case("FutureStatus", "Unknown")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn nonterminal_refresh_replaces_inflight_context_and_keeps_subscription(
    #[case] status: &str,
    #[case] expected_status: &str,
) {
    let (peer, http, mut observed, release) = refresh_peer(
        vec![
            EXCHANGES.into(),
            MARKET.into(),
            include_str!("../../test_data/poll-older.json").into(),
            metadata(status),
            ACTIVITY.into(),
        ],
        2,
    )
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 10_000).await;
    subscribe(&mut client, "activity").unwrap();

    for index in 0..3 {
        assert_eq!(observed.recv().await.unwrap(), index);
    }

    refresh(&client, false);
    assert_definition(response(&mut receiver).await, expected_status);
    release.send(()).unwrap();
    let mut data = Vec::new();

    for _ in 0..2 {
        let DataEvent::Data(Data::Quote(quote)) =
            time::timeout(Duration::from_secs(5), receiver.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("expected fresh quote")
        };

        data.push(quote);
    }

    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        data.iter()
            .map(|q| q.bid_price.to_string())
            .collect::<Vec<_>>(),
        ["0.3000", "0.3100"]
    );
    assert!(data.iter().all(|q| q.instrument_id == id()));
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case("Closed")]
#[case("Resolved")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reconnect_observes_terminal_before_restoring_retained_feeds(#[case] status: &str) {
    let (peer, http, mut observed, release) = refresh_peer(
        vec![
            EXCHANGES.into(),
            MARKET.into(),
            ACTIVITY.into(),
            EXCHANGES.into(),
            metadata(status),
        ],
        2,
    )
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    for feed in ["activity", "trade", "book", "depth", "bars"] {
        subscribe(&mut client, feed).unwrap();
    }

    for index in 0..3 {
        assert_eq!(observed.recv().await.unwrap(), index);
    }

    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
        panic!("expected terminal bootstrap")
    };

    assert_eq!(instrument.id(), id());
    assert_eq!(instrument.info().unwrap().get_str("status"), Some(status));
    release.send(()).unwrap();
    assert_eq!(peer.await.unwrap().len(), 5);
    client.disconnect().await.unwrap();
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn shutdown_cancels_refresh_without_deactivating_the_next_generation() {
    let (peer, http, mut observed, release) = refresh_peer(
        vec![
            EXCHANGES.into(),
            MARKET.into(),
            ACTIVITY.into(),
            metadata("Resolved"),
            EXCHANGES.into(),
            MARKET.into(),
            include_str!("../../test_data/poll-older.json").into(),
        ],
        3,
    )
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 10_000).await;
    subscribe(&mut client, "activity").unwrap();

    for _ in 0..2 {
        assert!(matches!(receiver.recv().await.unwrap(), DataEvent::Data(_)));
    }

    refresh(&client, false);

    for index in 0..4 {
        assert_eq!(observed.recv().await.unwrap(), index);
    }

    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
        panic!("expected open bootstrap")
    };

    assert_eq!(instrument.info().unwrap().get_str("status"), Some("Open"));
    release.send(()).unwrap();

    let DataEvent::Data(Data::Quote(quote)) =
        time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected resumed quote")
    };

    assert_eq!(quote.instrument_id, id());
    assert_eq!(quote.bid_price.to_string(), "0.2000");
    client.disconnect().await.unwrap();
    assert_eq!(peer.await.unwrap().len(), 7);
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn terminal_refresh_preserves_another_outcomes_inflight_poll() {
    let other_market = "OTHER_NO";
    let other_id = to_instrument_id("POLYMARKET", other_market).unwrap();
    let other = super::polling::second_definition(other_market);
    let (peer, http, mut observed, release) = refresh_peer(
        vec![
            EXCHANGES.into(),
            MARKET.into(),
            other,
            ACTIVITY.into(),
            metadata("Resolved"),
        ],
        3,
    )
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into(), other_market.into()], 10_000).await;
    client
        .subscribe_quotes(SubscribeQuotes::new(
            other_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
            None,
        ))
        .unwrap();

    for index in 0..4 {
        assert_eq!(observed.recv().await.unwrap(), index);
    }

    for feed in ["activity", "trade", "book", "depth", "bars"] {
        subscribe(&mut client, feed).unwrap();
    }

    refresh(&client, false);
    assert_definition(response(&mut receiver).await, "Resolved");
    release.send(()).unwrap();

    for expected in ["0.3000", "0.3100"] {
        let DataEvent::Data(Data::Quote(quote)) =
            time::timeout(Duration::from_secs(5), receiver.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("expected unaffected outcome quote")
        };

        assert_eq!(quote.instrument_id, other_id);
        assert_eq!(quote.bid_price.to_string(), expected);
    }

    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests[3].contains("/OTHER_NO/latest?limit=100"));
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case("status", Value::Null)]
#[case("status", serde_json::json!({"state": "Resolved"}))]
#[case("outcome_type", Value::String("Numeric".into()))]
#[case("outcome_type", Value::String("Unknown".into()))]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsupported_refresh_keeps_prior_definition_and_polling(
    #[case] field: &str,
    #[case] value: Value,
) {
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market[field] = value;
    let (peer, http, mut observed) = super::polling::timed_peer(vec![
        (200, EXCHANGES.to_owned(), Duration::ZERO),
        (200, MARKET.to_owned(), Duration::ZERO),
        (200, market.to_string(), Duration::ZERO),
        (200, MARKET.to_owned(), Duration::ZERO),
        (200, ACTIVITY.to_owned(), Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 10_000).await;
    refresh(&client, false);

    for index in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, index);
    }

    refresh(&client, false);
    assert_definition(response(&mut receiver).await, "Open");
    subscribe(&mut client, "activity").unwrap();

    for expected in ["0.3000", "0.3100"] {
        let DataEvent::Data(Data::Quote(quote)) =
            time::timeout(Duration::from_secs(5), receiver.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("expected quote after rejected refresh")
        };

        assert_eq!(quote.instrument_id, id());
        assert_eq!(quote.bid_price.to_string(), expected);
    }

    client.disconnect().await.unwrap();
    assert_eq!(peer.await.unwrap().len(), 5);
    assert!(receiver.try_recv().is_err());
}

fn refresh(client: &FinFeedDataClient, list: bool) {
    if list {
        client
            .request_instruments(RequestInstruments::new(
                None,
                None,
                None,
                Some(id().venue),
                UUID4::new(),
                UnixNanos::from(0),
                None,
            ))
            .unwrap();
    } else {
        client
            .request_instrument(RequestInstrument::new(
                id(),
                None,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
            ))
            .unwrap();
    }
}

fn assert_definition(response: DataResponse, status: &str) {
    let instrument = match response {
        DataResponse::Instrument(response) => {
            assert_eq!(response.instrument_id, id());
            response.data
        }
        DataResponse::Instruments(mut response) => {
            assert_eq!(response.data.len(), 1);
            response.data.remove(0)
        }
        _ => panic!("expected refreshed definition"),
    };

    assert_eq!(instrument.id(), id());
    assert_eq!(instrument.id().venue.as_str(), "FINFEED_POLYMARKET");
    assert_eq!(instrument.raw_symbol(), id().symbol);
    let info = instrument.info().unwrap();
    assert_eq!(info.get_str("status"), Some(status));
    assert_eq!(info.get_str("market_id"), Some(MARKET_ID));
    assert_eq!(info.get_str("outcome_type"), Some("Binary"));
    assert_eq!(info.get("settlement_price"), None);
    assert_eq!(info.get("resolution_time"), None);
}

fn subscribe(client: &mut FinFeedDataClient, feed: &str) -> anyhow::Result<()> {
    let ts = UnixNanos::from(0);

    match feed {
        "activity" => client.subscribe_quotes(SubscribeQuotes::new(
            id(),
            Some(client.client_id()),
            None,
            UUID4::new(),
            ts,
            None,
            None,
        )),
        "trade" => client.subscribe_trades(SubscribeTrades::new(
            id(),
            Some(client.client_id()),
            None,
            UUID4::new(),
            ts,
            None,
            None,
        )),
        "book" => client.subscribe_book_deltas(SubscribeBookDeltas::new(
            id(),
            BookType::L2_MBP,
            Some(client.client_id()),
            None,
            UUID4::new(),
            ts,
            None,
            false,
            None,
            None,
        )),
        "depth" => client.subscribe_book_depth(SubscribeBookDepth::new(
            id(),
            BookType::L2_MBP,
            Some(client.client_id()),
            None,
            UUID4::new(),
            ts,
            None,
            false,
            None,
            None,
        )),
        "periods" | "bars" => client.subscribe_bars(SubscribeBars::new(
            BarType::from(format!("{}-1-SECOND-LAST-EXTERNAL", id()).as_str()),
            Some(client.client_id()),
            None,
            UUID4::new(),
            ts,
            None,
            None,
        )),
        _ => unreachable!(),
    }
}

fn metadata(status: &str) -> String {
    // Synthetic lifecycle transitions omit authoritative settlement fields
    let mut market: Value = serde_json::from_str(MARKET).unwrap();
    market["status"] = status.into();
    market.to_string()
}

fn id() -> InstrumentId {
    to_instrument_id("POLYMARKET", MARKET_ID).unwrap()
}

async fn refresh_peer(
    replies: Vec<String>,
    blocked: usize,
) -> (
    task::JoinHandle<Vec<String>>,
    FinFeedHttpConfig,
    tokio::sync::mpsc::UnboundedReceiver<usize>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (listener, address) = bind_peer().await;

    let config = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        max_concurrent_requests: 2,
        ..Default::default()
    };

    let (observed, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let mut released = Some(released);

    let peer = task::spawn(async move {
        let mut requests = Vec::new();
        let mut delayed = None;

        for (index, body) in replies.into_iter().enumerate() {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut stream).await);
            observed.send(index).unwrap();
            if index == blocked {
                let release = released.take().unwrap();
                delayed = Some(task::spawn(async move {
                    let _ = release.await;
                    send_response(&mut stream, &body).await;
                }));
            } else {
                send_response(&mut stream, &body).await;
            }
        }

        if let Some(delayed) = delayed {
            delayed.await.unwrap();
        }

        assert!(
            time::timeout(Duration::from_millis(250), listener.accept())
                .await
                .is_err(),
            "unexpected poll after lifecycle update"
        );
        requests
    });

    (peer, config, receiver, release)
}

async fn send_response(stream: &mut TcpStream, body: &str) {
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
}
