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

//! Shared L2 snapshot polling, validation, depth, and subscription lifecycle.

use std::{num::NonZeroUsize, time::Duration};

use nautilus_common::{
    clients::DataClient,
    messages::{DataEvent, data::*},
};
use nautilus_core::{UUID4, UnixNanos, datetime::NANOSECONDS_IN_MILLISECOND};
use nautilus_finfeed::{common::symbol::to_instrument_id, data::FinFeedDataClient};
use nautilus_model::{
    data::{BookOrder, Data, NULL_ORDER, OrderBookDeltas, OrderBookDepth},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    orderbook::{OrderBook, analysis::book_check_integrity},
    types::{Price, Quantity},
};
use nautilus_network::dst::time;
use rstest::rstest;
use serde_json::Value;

use super::{
    dst::start_peer,
    polling::{connected_selection, timed_peer, wait_for_admission, wait_for_retry_backoff},
};

const EXCHANGES: &str = include_str!("../../test_data/exchanges.json");
const MARKET: &str = include_str!("../../test_data/polymarket-market.json");
const BOOK: &str = include_str!("../../test_data/poll-book.json");
const REPLACEMENT: &str = include_str!("../../test_data/poll-book-replacement.json");
const EMPTY: &str = include_str!("../../test_data/poll-book-empty.json");
const MARKET_ID: &str = "TEST-OUTCOME_NO";
const BIDS: &[(&str, &str)] = &[
    ("0.3000", "1.000001"),
    ("0.2000", "2.000002"),
    ("0.1000", "3.000003"),
];
const ASKS: &[(&str, &str)] = &[("0.4000", "4.000004"), ("0.5000", "5.000005")];

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn shared_snapshot_polls_replace_consumers_and_reject_invalid_duplicate_stale_books() {
    let mut duplicate: Value = serde_json::from_str(BOOK).unwrap();
    duplicate["bids"].as_array_mut().unwrap().reverse();
    let mut invalid: Value = serde_json::from_str(BOOK).unwrap();
    invalid["bids"][2]["price"] = Value::String("0.10001".into());
    invalid["time_exchange"] = Value::String("2026-10-11T00:00:06Z".into());
    invalid["time_coinapi"] = Value::String("2026-10-11T00:00:07Z".into());
    let mut stale: Value = serde_json::from_str(BOOK).unwrap();
    stale["time_exchange"] = Value::String("2026-10-11T00:00:00Z".into());
    stale["time_coinapi"] = Value::String("2026-10-11T00:00:01Z".into());
    let (peer, http, _) = timed_peer(vec![
        (200, EXCHANGES.to_owned(), Duration::ZERO),
        (200, MARKET.to_owned(), Duration::ZERO),
        (200, BOOK.to_owned(), Duration::ZERO),
        (200, duplicate.to_string(), Duration::ZERO),
        (200, invalid.to_string(), Duration::ZERO),
        (200, stale.to_string(), Duration::ZERO),
        (200, REPLACEMENT.to_owned(), Duration::ZERO),
        (200, EMPTY.to_owned(), Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    subscribe(&mut client, false, BookType::L2_MBP, Some(1)).unwrap();
    let mut full = OrderBook::new(id(), BookType::L2_MBP);
    let mut limited = OrderBook::new(id(), BookType::L2_MBP);

    for (bids, asks, times) in [
        (BIDS, ASKS, (1, 2)),
        (&[("0.2500", "6.000006")][..], &[][..], (3, 4)),
        (&[][..], &[][..], (5, 6)),
    ] {
        let deltas = assert_deltas(id(), event(&mut receiver).await, bids, asks, times);
        let depth = assert_depth(
            id(),
            event(&mut receiver).await,
            &bids[..bids.len().min(1)],
            &asks[..asks.len().min(1)],
            times,
        );
        full.apply_deltas(&deltas).unwrap();
        limited.apply_depth(&depth).unwrap();
        assert_state(id(), &full, bids, asks);
        assert_state(
            id(),
            &limited,
            &bids[..bids.len().min(1)],
            &asks[..asks.len().min(1)],
        );
    }

    client.disconnect().await.unwrap();
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 8);

    for request in &requests[2..] {
        assert_current(request);
    }

    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(true)]
#[case(false)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn adding_a_feed_bootstraps_unchanged_snapshot_and_partial_unsubscribe_keeps_the_other(
    #[case] deltas_first: bool,
) {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", BOOK),
        (200, "", BOOK),
        (200, "", BOOK),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;
    subscribe(&mut client, deltas_first, BookType::L2_MBP, Some(1)).unwrap();
    let first = event(&mut receiver).await;
    subscribe(&mut client, !deltas_first, BookType::L2_MBP, Some(1)).unwrap();
    let both = [event(&mut receiver).await, event(&mut receiver).await];
    unsubscribe(&mut client, deltas_first);
    let remaining = event(&mut receiver).await;
    client.disconnect().await.unwrap();

    if deltas_first {
        assert_deltas(id(), first, &BIDS[..1], &ASKS[..1], (1, 2));
        assert_depth(id(), remaining, &BIDS[..1], &ASKS[..1], (1, 2));
    } else {
        assert_depth(id(), first, &BIDS[..1], &ASKS[..1], (1, 2));
        assert_deltas(id(), remaining, &BIDS[..1], &ASKS[..1], (1, 2));
    }

    let [deltas, depth] = both;
    assert_deltas(id(), deltas, &BIDS[..1], &ASKS[..1], (1, 2));
    assert_depth(id(), depth, &BIDS[..1], &ASKS[..1], (1, 2));
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

#[rstest]
#[case(true, None, 3)]
#[case(true, Some(1), 1)]
#[case(true, Some(20), 3)]
#[case(false, None, 3)]
#[case(false, Some(1), 1)]
#[case(false, Some(20), 3)]
#[case(false, Some(usize::MAX), 3)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn depth_limits_return_available_levels_and_duplicate_commands_are_idempotent(
    #[case] deltas: bool,
    #[case] depth: Option<usize>,
    #[case] count: usize,
) {
    let (peer, http, _) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, BOOK, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;
    subscribe(&mut client, deltas, BookType::L2_MBP, depth).unwrap();
    subscribe(&mut client, deltas, BookType::L2_MBP, depth).unwrap();
    let data = event(&mut receiver).await;
    unsubscribe(&mut client, deltas);
    unsubscribe(&mut client, deltas);
    client.disconnect().await.unwrap();

    if deltas {
        assert_deltas(
            id(),
            data,
            &BIDS[..count],
            &ASKS[..ASKS.len().min(count)],
            (1, 2),
        );
    } else {
        assert_depth(
            id(),
            data,
            &BIDS[..count],
            &ASKS[..ASKS.len().min(count)],
            (1, 2),
        );
    }

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

#[rstest]
#[case(true, BookType::L1_MBP)]
#[case(true, BookType::L3_MBO)]
#[case(false, BookType::L1_MBP)]
#[case(false, BookType::L3_MBO)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsupported_book_types_fail_before_io(#[case] deltas: bool, #[case] book_type: BookType) {
    let (peer, http, _) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    let error = subscribe(&mut client, deltas, book_type, None).unwrap_err();
    client.disconnect().await.unwrap();
    assert_eq!(
        error.to_string(),
        "FinFeed book subscriptions require L2_MBP"
    );
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn default_depth_is_idempotent_after_publication_and_reconfiguration_bootstraps() {
    let (peer, http, _) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, BOOK, Duration::ZERO),
        (200, BOOK, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;
    subscribe(&mut client, false, BookType::L2_MBP, None).unwrap();
    let first = event(&mut receiver).await;
    subscribe(&mut client, false, BookType::L2_MBP, Some(10)).unwrap();
    assert!(
        time::timeout(Duration::from_millis(350), receiver.recv())
            .await
            .is_err()
    );
    subscribe(&mut client, false, BookType::L2_MBP, Some(1)).unwrap();
    let limited = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_depth(id(), first, BIDS, ASKS, (1, 2));
    assert_depth(id(), limited, &BIDS[..1], &ASKS[..1], (1, 2));
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
#[case(BOOK, BIDS, ASKS, (1, 2))]
#[case(REPLACEMENT, &[("0.2500", "6.000006")], &[], (3, 4))]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reconnect_fetches_a_new_snapshot_even_if_unchanged_or_older(
    #[case] initial: &'static str,
    #[case] bids: &[(&str, &str)],
    #[case] asks: &[(&str, &str)],
    #[case] times: (u64, u64),
) {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", initial),
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", BOOK),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    let old = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    assert!(receiver.try_recv().is_err());
    let fresh = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_deltas(id(), old, bids, asks, times);
    let fresh = assert_deltas(id(), fresh, BIDS, ASKS, (1, 2));
    let mut consumer = OrderBook::new(id(), BookType::L2_MBP);
    consumer.apply_deltas(&fresh).unwrap();
    assert_state(id(), &consumer, BIDS, ASKS);
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 6);
    assert_current(&requests[2]);
    assert_current(&requests[5]);
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsubscribe_or_disconnect_cancels_inflight_book_without_late_publication(
    #[case] disconnect: bool,
) {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, BOOK, Duration::from_secs(2)),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    subscribe(&mut client, false, BookType::L2_MBP, None).unwrap();

    for expected in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    if disconnect {
        client.disconnect().await.unwrap();
    } else {
        unsubscribe(&mut client, true);
        unsubscribe(&mut client, false);
    }

    assert!(
        time::timeout(Duration::from_millis(350), receiver.recv())
            .await
            .is_err()
    );
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
async fn partial_unsubscribe_drops_old_request_before_starting_fresh_remaining_feed() {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, BOOK, Duration::from_secs(2)),
        (200, REPLACEMENT, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    subscribe(&mut client, false, BookType::L2_MBP, Some(1)).unwrap();

    for expected in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    unsubscribe(&mut client, false);
    let (index, _) = time::timeout(Duration::from_secs(1), observed.recv())
        .await
        .expect("fresh request must precede stale response")
        .unwrap();
    let data = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_eq!(index, 3);
    assert_deltas(id(), data, &[("0.2500", "6.000006")], &[], (3, 4));
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
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn endpoint_churn_does_not_cancel_the_other_inflight_endpoint(#[case] book_inflight: bool) {
    let source = if book_inflight {
        BOOK
    } else {
        include_str!("../../test_data/poll-activity.json")
    };

    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, source, Duration::from_millis(200)),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;

    if book_inflight {
        subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    } else {
        super::polling::subscribe(&mut client, true, false);
    }

    for expected in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    for _ in 0..8 {
        if book_inflight {
            super::polling::subscribe(&mut client, true, false);
            super::polling::unsubscribe(&mut client, true, false);
        } else {
            subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
            unsubscribe(&mut client, true);
        }

        time::sleep(Duration::from_millis(10)).await;
    }

    let first = event(&mut receiver).await;
    if book_inflight {
        assert_deltas(id(), first, BIDS, ASKS, (1, 2));
    } else {
        let Data::Quote(first) = first else {
            panic!("expected surviving quote")
        };

        let Data::Quote(second) = event(&mut receiver).await else {
            panic!("expected surviving quote")
        };

        assert_eq!(first.instrument_id, id());
        assert_eq!(first.bid_price.to_string(), "0.3000");
        assert_eq!(second.bid_price.to_string(), "0.3100");
    }

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
async fn activity_and_book_endpoints_poll_serially_by_independent_due_times() {
    let (peer, http, _) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (
            200,
            include_str!("../../test_data/poll-activity.json"),
            Duration::ZERO,
        ),
        (200, BOOK, Duration::ZERO),
        (
            200,
            include_str!("../../test_data/poll-older.json"),
            Duration::ZERO,
        ),
        (200, REPLACEMENT, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 250).await;
    super::polling::subscribe(&mut client, true, false);
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    let events = [
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
    ];
    client.disconnect().await.unwrap();
    let [quote1, quote2, book1, quote3, book2] = events;
    for quote in [quote1, quote2, quote3] {
        let Data::Quote(quote) = quote else {
            panic!("expected sampled quote")
        };

        assert_eq!(quote.instrument_id, id());
    }

    assert_deltas(id(), book1, BIDS, ASKS, (1, 2));
    assert_deltas(id(), book2, &[("0.2500", "6.000006")], &[], (3, 4));
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 6);

    for request in [&requests[3], &requests[5]] {
        assert_current(request);
    }

    for request in [&requests[2], &requests[4]] {
        assert_eq!(
            request.lines().next().unwrap(),
            format!("GET /v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=100 HTTP/1.1")
        );
    }

    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn book_attempt_budget_stops_wire_work_and_reconnect_restores_intent() {
    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, BOOK, Duration::ZERO),
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, REPLACEMENT, Duration::ZERO),
    ])
    .await;
    http.attempt_budget = 3;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    let first = event(&mut receiver).await;
    time::sleep(Duration::from_millis(350)).await;

    for expected in 0..3 {
        assert_eq!(observed.try_recv().unwrap().0, expected);
    }

    assert!(observed.try_recv().is_err());
    assert!(receiver.try_recv().is_err());
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    let fresh = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_deltas(id(), first, BIDS, ASKS, (1, 2));
    assert_deltas(id(), fresh, &[("0.2500", "6.000006")], &[], (3, 4));
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        6
    );
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(None, 10, 10)]
#[case(Some(11), 11, 11)]
#[case(Some(20), 20, 13)]
#[case(Some(usize::MAX), 20, 13)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn depth_above_inline_capacity_keeps_sorted_available_levels(
    #[case] requested: Option<usize>,
    #[case] bids_count: usize,
    #[case] asks_count: usize,
) {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", include_str!("../../test_data/poll-book-deep.json")),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;
    subscribe(&mut client, false, BookType::L2_MBP, requested).unwrap();
    let data = event(&mut receiver).await;
    client.disconnect().await.unwrap();

    let bids: Vec<_> = (0..bids_count)
        .map(|index| {
            (
                format!("0.{:04}", 3000 - index * 10),
                format!("{}.{:06}", index + 1, index + 1),
            )
        })
        .collect();

    let asks: Vec<_> = (0..asks_count)
        .map(|index| {
            (
                format!("0.{:04}", 4000 + index * 10),
                format!("{}.{:06}", 101 + index, 101 + index),
            )
        })
        .collect();

    let bids: Vec<_> = bids
        .iter()
        .map(|(price, size)| (price.as_str(), size.as_str()))
        .collect();
    let asks: Vec<_> = asks
        .iter()
        .map(|(price, size)| (price.as_str(), size.as_str()))
        .collect();
    let depth = assert_depth(id(), data, &bids, &asks, (1, 2));
    let mut consumer = OrderBook::new(id(), BookType::L2_MBP);
    consumer.apply_depth(&depth).unwrap();
    assert_state(id(), &consumer, &bids, &asks);
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
async fn transient_errors_and_invalid_wire_book_recover_on_later_poll() {
    let mut malformed: Value = serde_json::from_str(BOOK).unwrap();
    malformed["time_coinapi"] = Value::String("invalid-time".into());
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (503, "", "{}".to_owned()),
        (503, "", "{}".to_owned()),
        (503, "", "{}".to_owned()),
        (404, "", "{}".to_owned()),
        (200, "", malformed.to_string()),
        (200, "", BOOK.to_owned()),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    let data = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_deltas(id(), data, BIDS, ASKS, (1, 2));
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 8);

    for request in &requests[2..] {
        assert_current(request);
    }

    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(401)]
#[case(403)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn permanent_book_error_stays_suspended_through_partial_unsubscribe(#[case] status: u16) {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (status, "{}", Duration::ZERO),
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, BOOK, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    subscribe(&mut client, false, BookType::L2_MBP, Some(1)).unwrap();

    for expected in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    time::sleep(Duration::from_millis(350)).await;
    unsubscribe(&mut client, false);
    time::sleep(Duration::from_millis(350)).await;
    assert!(observed.try_recv().is_err());
    assert!(receiver.try_recv().is_err());
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    let data = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_deltas(id(), data, BIDS, ASKS, (1, 2));
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        6
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn book_retries_obey_retry_after_and_shared_admission_pacing() {
    #[cfg(all(feature = "simulation", madsim))]
    madsim::net::NetSim::current().update_config(|config| {
        config.send_latency = Duration::ZERO..Duration::from_nanos(1);
    });

    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (429, "{}", Duration::ZERO),
        (200, BOOK, Duration::ZERO),
    ])
    .await;
    let interval_ms = 150;
    http.min_request_interval_ms = interval_ms;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    let data = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    let mut times = Vec::new();
    while let Ok((_, time)) = observed.try_recv() {
        times.push(time);
    }

    assert_deltas(id(), data, BIDS, ASKS, (1, 2));
    assert_eq!(requests.len(), 4);
    assert_eq!(times.len(), 4);
    assert!(times[3].duration_since(times[2]) >= Duration::from_secs(1));

    let minimum: u64 = if cfg!(all(feature = "simulation", madsim)) {
        interval_ms
    } else {
        100
    };

    for pair in times.windows(2) {
        let spacing = pair[1].duration_since(pair[0]);
        let nanos_per_ms = u128::from(NANOSECONDS_IN_MILLISECOND);

        // Peer receipt includes transport scheduling jitter; compare at millisecond resolution
        let millis = (spacing.as_nanos() + nanos_per_ms / 2) / nanos_per_ms;
        assert!(
            millis >= u128::from(minimum),
            "peer spacing was {spacing:?}"
        );
    }

    assert_current(&requests[2]);
    assert_current(&requests[3]);
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn two_book_instruments_poll_by_due_time_and_publish_only_their_requested_feed() {
    let second_id = "other_NO";
    let second = super::polling::second_definition(second_id);
    let mut second_book: Value = serde_json::from_str(BOOK).unwrap();
    second_book["market_id"] = Value::String(second_id.into());
    let mut second_replacement: Value = serde_json::from_str(REPLACEMENT).unwrap();
    second_replacement["market_id"] = Value::String(second_id.into());
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", second),
        (200, "", BOOK.to_owned()),
        (200, "", second_book.to_string()),
        (200, "", REPLACEMENT.to_owned()),
        (200, "", second_replacement.to_string()),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into(), second_id.into()], 250).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    let second_instrument = to_instrument_id("POLYMARKET", second_id).unwrap();
    client
        .subscribe_book_depth(SubscribeBookDepth::new(
            second_instrument,
            BookType::L2_MBP,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            NonZeroUsize::new(20),
            false,
            None,
            None,
        ))
        .unwrap();
    let events = [
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
    ];
    client.disconnect().await.unwrap();
    let [first, second, first_replacement, second_replacement] = events;
    assert_deltas(id(), first, BIDS, ASKS, (1, 2));
    assert_depth(second_instrument, second, BIDS, ASKS, (1, 2));
    assert_deltas(
        id(),
        first_replacement,
        &[("0.2500", "6.000006")],
        &[],
        (3, 4),
    );
    assert_depth(
        second_instrument,
        second_replacement,
        &[("0.2500", "6.000006")],
        &[],
        (3, 4),
    );
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 7);
    let paths: Vec<_> = requests[3..]
        .iter()
        .map(|request| request.lines().next().unwrap())
        .collect();
    assert_eq!(
        paths,
        [MARKET_ID, second_id, MARKET_ID, second_id]
            .map(|market| format!("GET /v1/orderbook/POLYMARKET/{market}/current HTTP/1.1"))
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reset_clears_book_intent_before_reconnect() {
    let (peer, http, _) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, BOOK, Duration::ZERO),
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    let data = event(&mut receiver).await;
    client.reset().unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    assert!(
        time::timeout(Duration::from_millis(350), receiver.recv())
            .await
            .is_err()
    );
    client.disconnect().await.unwrap();
    assert_deltas(id(), data, BIDS, ASKS, (1, 2));
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
async fn unsubscribe_cancels_book_retry_backoff_without_a_second_attempt() {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (429, "{}", Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;
    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();

    for expected in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    wait_for_retry_backoff(&client).await;
    unsubscribe(&mut client, true);
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

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsubscribe_cancels_book_admission_wait_before_any_poll_is_sent() {
    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
    ])
    .await;
    http.min_request_interval_ms = 200;
    let (mut client, mut receiver) = connected_selection(http, vec![MARKET_ID.into()], 100).await;

    subscribe(&mut client, true, BookType::L2_MBP, None).unwrap();
    wait_for_admission(&client).await;
    unsubscribe(&mut client, true);
    time::sleep(Duration::from_millis(250)).await;
    client.disconnect().await.unwrap();
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(observed.try_recv().unwrap().0, 0);
    assert_eq!(observed.try_recv().unwrap().0, 1);
    assert!(observed.try_recv().is_err());
    assert!(receiver.try_recv().is_err());
}

fn subscribe(
    client: &mut FinFeedDataClient,
    deltas: bool,
    book_type: BookType,
    depth: Option<usize>,
) -> anyhow::Result<()> {
    let client_id = Some(client.client_id());
    let depth = depth.map(|depth| NonZeroUsize::new(depth).unwrap());

    if deltas {
        client.subscribe_book_deltas(SubscribeBookDeltas::new(
            id(),
            book_type,
            client_id,
            None,
            UUID4::new(),
            UnixNanos::from(0),
            depth,
            false,
            None,
            None,
        ))
    } else {
        client.subscribe_book_depth(SubscribeBookDepth::new(
            id(),
            book_type,
            client_id,
            None,
            UUID4::new(),
            UnixNanos::from(0),
            depth,
            false,
            None,
            None,
        ))
    }
}

fn unsubscribe(client: &mut FinFeedDataClient, deltas: bool) {
    let client_id = Some(client.client_id());
    if deltas {
        client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                id(),
                client_id,
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None,
            ))
            .unwrap();
    } else {
        client
            .unsubscribe_book_depth(&UnsubscribeBookDepth::new(
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

fn id() -> InstrumentId {
    to_instrument_id("POLYMARKET", MARKET_ID).unwrap()
}

async fn event(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Data {
    let DataEvent::Data(data) = time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected book publication")
    };

    data
}

fn assert_deltas(
    instrument_id: InstrumentId,
    data: Data,
    bids: &[(&str, &str)],
    asks: &[(&str, &str)],
    times: (u64, u64),
) -> OrderBookDeltas {
    let Data::BookDeltas(deltas) = data else {
        panic!("expected snapshot deltas")
    };

    assert_eq!(deltas.instrument_id, instrument_id);
    assert_eq!(deltas.sequence, 0);
    assert_eq!(deltas.ts_event, timestamp(times.0));
    assert_eq!(deltas.ts_init, timestamp(times.1));
    assert_eq!(
        deltas.flags,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
    );
    assert_eq!(deltas.deltas.len(), 1 + bids.len() + asks.len());
    let clear = &deltas.deltas[0];
    assert_eq!(clear.action, BookAction::Clear);
    assert_eq!(clear.order.side, NULL_ORDER.side);
    assert_eq!(clear.order.price, NULL_ORDER.price);
    assert_eq!(clear.order.price.precision, NULL_ORDER.price.precision);
    assert_eq!(clear.order.size, NULL_ORDER.size);
    assert_eq!(clear.order.size.precision, NULL_ORDER.size.precision);
    assert_eq!(clear.order.order_id, NULL_ORDER.order_id);
    let expected = bids
        .iter()
        .map(|level| (OrderSide::Buy, level))
        .chain(asks.iter().map(|level| (OrderSide::Sell, level)));

    for (delta, (side, (price, size))) in deltas.deltas[1..].iter().zip(expected) {
        assert_eq!(delta.action, BookAction::Add);
        assert_order(&delta.order, side, price, size);
    }

    for (index, delta) in deltas.deltas.iter().enumerate() {
        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.sequence, 0);
        assert_eq!(delta.ts_event, timestamp(times.0));
        assert_eq!(delta.ts_init, timestamp(times.1));

        let last = if index + 1 == deltas.deltas.len() {
            RecordFlag::F_LAST as u8
        } else {
            0
        };

        assert_eq!(delta.flags, RecordFlag::F_SNAPSHOT as u8 | last);
    }

    *deltas
}

fn assert_depth(
    instrument_id: InstrumentId,
    data: Data,
    bids: &[(&str, &str)],
    asks: &[(&str, &str)],
    times: (u64, u64),
) -> OrderBookDepth {
    let Data::BookDepth(depth) = data else {
        panic!("expected depth snapshot")
    };

    assert_eq!(depth.instrument_id, instrument_id);
    assert_eq!(depth.sequence, 0);
    assert_eq!(depth.ts_event, timestamp(times.0));
    assert_eq!(depth.ts_init, timestamp(times.1));
    assert_eq!(
        depth.flags,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
    );
    assert_eq!(depth.bids.len(), bids.len());
    assert_eq!(depth.asks.len(), asks.len());
    assert_eq!(depth.bid_counts.as_slice(), vec![0; bids.len()]);
    assert_eq!(depth.ask_counts.as_slice(), vec![0; asks.len()]);

    for (order, (price, size)) in depth.bids.iter().zip(bids) {
        assert_order(order, OrderSide::Buy, price, size);
    }

    for (order, (price, size)) in depth.asks.iter().zip(asks) {
        assert_order(order, OrderSide::Sell, price, size);
    }

    *depth
}

fn assert_order(order: &BookOrder, side: OrderSide, price: &str, size: &str) {
    assert_eq!(order.side, Some(side));
    assert_eq!(order.price.to_string(), price);
    assert_eq!(order.price.precision, 4);
    assert_eq!(order.size.to_string(), size);
    assert_eq!(order.size.precision, 6);
    assert_eq!(order.order_id, 0);
}

fn assert_state(
    instrument_id: InstrumentId,
    book: &OrderBook,
    bids: &[(&str, &str)],
    asks: &[(&str, &str)],
) {
    assert_eq!(book.instrument_id, instrument_id);
    assert_eq!(book.book_type, BookType::L2_MBP);
    book_check_integrity(book).unwrap();

    let levels = |levels: &[(&str, &str)]| {
        levels
            .iter()
            .map(|(price, size)| {
                (
                    Price::from(*price).as_decimal(),
                    Quantity::from(*size).as_decimal(),
                )
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(
        book.bids_as_map(None).into_iter().collect::<Vec<_>>(),
        levels(bids)
    );
    assert_eq!(
        book.asks_as_map(None).into_iter().collect::<Vec<_>>(),
        levels(asks)
    );
    assert_eq!(
        book.best_bid_price(),
        bids.first().map(|(price, _)| Price::from(*price))
    );
    assert_eq!(
        book.best_ask_price(),
        asks.first().map(|(price, _)| Price::from(*price))
    );
}

fn timestamp(seconds: u64) -> UnixNanos {
    UnixNanos::from(1_791_676_800_000_000_000 + seconds * 1_000_000_000)
}

fn assert_current(request: &str) {
    assert_eq!(
        request.lines().next().unwrap(),
        format!("GET /v1/orderbook/POLYMARKET/{MARKET_ID}/current HTTP/1.1")
    );
}
