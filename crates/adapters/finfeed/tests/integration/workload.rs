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

//! Attempt budgets, bootstrap deadlines, and polling costs across collection sizes.

use std::{collections::BTreeMap, time::Duration};

#[cfg(all(feature = "simulation", madsim))]
use nautilus_common::messages::DataEvent;
use nautilus_common::{clients::DataClient, messages::data::*};
#[cfg(all(feature = "simulation", madsim))]
use nautilus_core::string::secret::SecretString;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_finfeed::{
    common::symbol::to_instrument_id, data::FinFeedDataClient, http::client::FinFeedHttpConfig,
};
#[cfg(all(feature = "simulation", madsim))]
use nautilus_finfeed::{
    config::FinFeedDataClientConfig, http::error::Error, instruments::FinFeedInstrumentSelection,
};
use nautilus_model::{data::BarType, enums::BookType, identifiers::InstrumentId};
#[cfg(all(feature = "simulation", madsim))]
use nautilus_model::{data::Data, identifiers::ClientId, instruments::Instrument};
use nautilus_network::dst::{task, time};
#[cfg(all(feature = "simulation", madsim))]
use nautilus_network::retry::RetryError;
use rstest::rstest;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::{
    dst::{bind_peer, read_request},
    polling::connected_selection,
};

#[cfg(all(feature = "simulation", madsim))]
#[rstest]
#[case(false)]
#[case(true)]
#[madsim::test]
async fn day_long_allowance_exhaustion_and_cancellation(#[case] cancel: bool) {
    madsim::net::NetSim::current().update_config(|config| {
        config.send_latency = Duration::ZERO..Duration::from_nanos(1);
    });

    let successes = 160;
    let mut replies = vec![
        (
            200,
            include_str!("../../test_data/exchanges.json"),
            Duration::ZERO
        );
        successes
    ];

    if !cancel {
        replies.push((
            503,
            include_str!("../../test_data/empty.json"),
            Duration::ZERO,
        ));
    }

    let (peer, mut config, mut observed) = super::polling::timed_peer(replies).await;
    config.attempt_budget = successes as u64 + 1;
    config.min_request_interval_ms = 600_000;
    config.retry.max_retries = 0;
    config.retry.operation_timeout_ms = Some(660_000);
    config.retry.max_elapsed_ms = Some(720_000);
    let client = std::sync::Arc::new(super::dst::new_client(config));
    let started = time::Instant::now();

    for index in 0..successes {
        let exchanges = client.get_exchanges().await.unwrap();
        assert_eq!(exchanges.len(), 9);
        assert_eq!(client.attempts_remaining(), (successes - index) as u64);
    }

    if cancel {
        let pending = task::spawn({
            let client = std::sync::Arc::clone(&client);
            async move { client.get_exchanges().await }
        });

        time::sleep(Duration::from_secs(1)).await;
        client.cancel();
        assert!(matches!(
            pending.await.unwrap(),
            Err(Error::Retry(RetryError::Canceled))
        ));
        assert_eq!(client.attempts_remaining(), 1);
    } else {
        assert!(matches!(
            client.get_exchanges().await,
            Err(Error::Http { status: 503, .. })
        ));
        assert_eq!(client.attempts_remaining(), 0);

        for _ in 0..2 {
            assert!(matches!(
                client.get_exchanges().await,
                Err(Error::AttemptBudget)
            ));
        }
    }

    let requests = peer.await.unwrap();
    let expected = successes + usize::from(!cancel);
    assert_eq!(requests.len(), expected);
    assert!(started.elapsed() >= Duration::from_secs(86_400));

    for index in 0..expected {
        let (actual_index, received) = observed.try_recv().unwrap();
        assert_eq!(actual_index, index);
        assert!(received.duration_since(started) >= Duration::from_secs(index as u64 * 600));
    }

    assert!(observed.try_recv().is_err());
}

#[cfg(all(feature = "simulation", madsim))]
#[madsim::test]
async fn day_long_polling_exhausts_shared_allowance_and_reconnects_deliberately() {
    let allowance = 150;
    let market_id = "TEST-OUTCOME_NO";
    let exchanges = include_str!("../../test_data/exchanges.json");
    let market = include_str!("../../test_data/polymarket-market.json");
    let activity = include_str!("../../test_data/poll-activity.json");
    let mut replies = vec![
        (200, exchanges, Duration::ZERO),
        (200, market, Duration::ZERO),
    ];
    replies.extend(vec![(200, activity, Duration::ZERO); allowance - 2]);
    replies.extend([
        (200, exchanges, Duration::ZERO),
        (200, market, Duration::ZERO),
        (
            200,
            include_str!("../../test_data/poll-older.json"),
            Duration::ZERO,
        ),
    ]);
    let (peer, mut http, mut observed) = super::polling::timed_peer(replies).await;
    http.attempt_budget = allowance as u64;
    http.min_request_interval_ms = 10_000;
    http.retry.max_retries = 0;
    http.retry.operation_timeout_ms = Some(30_000);
    http.retry.max_elapsed_ms = Some(60_000);
    let (mut client, mut receiver) =
        connected_selection(http, vec![market_id.into()], 600_000).await;
    let started = time::Instant::now();
    super::polling::subscribe(&mut client, true, true);

    for index in 0..allowance {
        let (actual_index, _) = time::timeout(Duration::from_secs(620), observed.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual_index, index);
    }

    assert!(started.elapsed() >= Duration::from_secs(86_400));
    time::sleep(Duration::from_secs(1_220)).await;
    assert!(observed.try_recv().is_err());
    assert!(client.is_connected());
    client.connect().await.unwrap();
    super::polling::subscribe(&mut client, true, true);
    time::sleep(Duration::from_secs(620)).await;
    assert!(observed.try_recv().is_err());

    for _ in 0..4 {
        super::polling::event(&mut receiver).await;
    }

    assert!(receiver.try_recv().is_err());
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
        panic!("expected reconnect definition")
    };

    assert_eq!(
        instrument.id(),
        to_instrument_id("POLYMARKET", market_id).unwrap()
    );

    for index in allowance..allowance + 3 {
        let (actual_index, _) = time::timeout(Duration::from_secs(40), observed.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual_index, index);
    }

    let events = vec![
        super::polling::event(&mut receiver).await,
        super::polling::event(&mut receiver).await,
    ];
    assert_eq!(
        events,
        vec![
            super::polling::quote("0.2", "0.3", "5.000005", "6.000006", 0, 1),
            super::polling::trade("trade-2", "0.21", "3.000003", 1)
        ]
    );
    client.disconnect().await.unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), allowance + 3);

    for index in [0, allowance] {
        assert_eq!(
            requests[index].lines().next().unwrap(),
            "GET /v1/exchanges HTTP/1.1"
        );
        assert_eq!(
            requests[index + 1].lines().next().unwrap(),
            format!("GET /v1/markets/POLYMARKET/{market_id} HTTP/1.1")
        );
    }

    for request in requests[2..allowance]
        .iter()
        .chain(&requests[allowance + 2..])
    {
        assert_eq!(
            request.lines().next().unwrap(),
            format!("GET /v1/activity/POLYMARKET/{market_id}/latest?limit=100 HTTP/1.1")
        );
    }
}

#[cfg(all(feature = "simulation", madsim))]
#[rstest]
#[case(12, 120, false)]
#[case(128, 120, false)]
#[case(128, 1_500, true)]
#[madsim::test]
async fn multi_outcome_bootstrap_sizes_allowance_and_whole_deadline(
    #[case] outcomes: usize,
    #[case] deadline: u64,
    #[case] succeeds: bool,
) {
    madsim::net::NetSim::current().update_config(|config| {
        config.send_latency = Duration::ZERO..Duration::from_nanos(1);
    });

    let ids: Vec<_> = (0..outcomes)
        .map(|index| format!("workload-{index}_NO"))
        .collect();
    let mut replies = vec![(200, response("/v1/exchanges"), Duration::ZERO)];
    for id in &ids {
        replies.push((
            200,
            response(&format!("/v1/markets/POLYMARKET/{id}")),
            Duration::ZERO,
        ));
    }

    let (peer, mut http, mut observed) = super::polling::timed_peer(replies).await;
    http.attempt_budget = (outcomes + 1) as u64;
    http.min_request_interval_ms = 10_000;
    http.retry.max_retries = 0;
    http.retry.operation_timeout_ms = Some(30_000);
    http.retry.max_elapsed_ms = Some(60_000);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http,
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: ids.iter().map(|id| id.as_str().into()).collect(),
        }],
        connect_timeout_secs: deadline,
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    let started = time::Instant::now();
    let result = client.connect().await;
    if succeeds {
        result.unwrap();
        assert!(client.is_connected());

        for id in &ids {
            let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
                panic!("expected startup definition")
            };

            assert_eq!(instrument.id(), to_instrument_id("POLYMARKET", id).unwrap());
        }

        assert!(started.elapsed() >= Duration::from_secs(outcomes as u64 * 10));
        assert!(started.elapsed() < Duration::from_secs(deadline));
        let requests = peer.await.unwrap();
        assert_eq!(requests.len(), outcomes + 1);
    } else {
        assert_eq!(
            result.unwrap_err().to_string(),
            "FinFeed bootstrap timed out"
        );
        assert!(client.is_disconnected());
        assert!(started.elapsed() >= Duration::from_secs(deadline));
        peer.abort();
    }

    let mut count = 0;
    while observed.try_recv().is_ok() {
        count += 1;
    }

    assert_eq!(count, if succeeds { outcomes + 1 } else { 12 });
    assert!(receiver.try_recv().is_err());
    client.disconnect().await.unwrap();
}

#[rstest]
#[case(1)]
#[case(16)]
#[case(128)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn polling_workload_shares_feeds_and_bounds_fanout(#[case] markets: usize) {
    #[cfg(all(feature = "simulation", madsim))]
    madsim::net::NetSim::current().update_config(|config| {
        config.send_latency = Duration::ZERO..Duration::from_nanos(1);
    });

    let (listener, address) = bind_peer().await;
    let attempts = 1 + 8 * markets;
    let (observed, mut requests) = tokio::sync::mpsc::unbounded_channel();

    let peer = task::spawn(async move {
        for _ in 0..attempts {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let target = request.split_whitespace().nth(1).unwrap();
            let started = time::Instant::now();
            let body = response(target);
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
            observed.send((target.to_owned(), started)).unwrap();
        }
    });

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        attempt_budget: attempts as u64,
        ..Default::default()
    };

    let ids: Vec<_> = (0..markets)
        .map(|index| format!("workload-{index}_NO"))
        .collect();
    let started = time::Instant::now();
    let (mut client, _receiver) = connected_selection(http, ids.clone(), 5_000).await;
    for market in &ids {
        subscribe(&mut client, to_instrument_id("POLYMARKET", market).unwrap());
    }

    time::timeout(Duration::from_secs(30), peer)
        .await
        .unwrap()
        .unwrap();
    client.disconnect().await.unwrap();
    let elapsed = started.elapsed();
    let mut counts: BTreeMap<String, Vec<time::Instant>> = BTreeMap::new();
    #[cfg(all(feature = "simulation", madsim))]
    let mut previous = None;
    let mut actual = 0;

    while let Ok((target, time)) = requests.try_recv() {
        #[cfg(all(feature = "simulation", madsim))]
        {
            if let Some(previous) = previous {
                assert!(time.duration_since(previous) >= Duration::from_millis(1));
            }

            previous = Some(time);
        }

        counts.entry(target).or_default().push(time);
        actual += 1;
    }

    assert_eq!(actual, attempts);
    assert_eq!(counts["/v1/exchanges"].len(), 1);
    assert_eq!(counts["/v1/ohlcv/periods"].len(), markets);

    for market in ids {
        assert_eq!(counts[&format!("/v1/markets/POLYMARKET/{market}")].len(), 1);

        for target in [
            format!("/v1/activity/POLYMARKET/{market}/latest?limit=100"),
            format!("/v1/orderbook/POLYMARKET/{market}/current"),
            format!("/v1/ohlcv/POLYMARKET/{market}/latest?period_id=1MIN&limit=101"),
        ] {
            let times = &counts[&target];
            assert_eq!(times.len(), 2, "{target}");
            assert!(times[1].duration_since(times[0]) >= Duration::from_millis(5_000));
        }
    }

    println!(
        "markets={markets}, managed_calls={actual}, elapsed={elapsed:?}, shared_spacing_ms=1, endpoint_interval_ms=5000"
    );
}

fn subscribe(client: &mut FinFeedDataClient, id: InstrumentId) {
    let client_id = Some(client.client_id());
    client
        .subscribe_quotes(SubscribeQuotes::new(
            id,
            client_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_trades(SubscribeTrades::new(
            id,
            client_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            id,
            BookType::L2_MBP,
            client_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_book_depth(SubscribeBookDepth::new(
            id,
            BookType::L2_MBP,
            client_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .unwrap();
    let bar_type = BarType::from(format!("{id}-1-MINUTE-LAST-EXTERNAL"));
    client
        .subscribe_bars(SubscribeBars::new(
            bar_type,
            client_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
}

fn response(target: &str) -> String {
    if target == "/v1/exchanges" {
        return include_str!("../../test_data/exchanges.json").into();
    }

    if target == "/v1/ohlcv/periods" {
        return include_str!("../../test_data/periods.json").into();
    }

    let market_id = target.split('/').nth(4).unwrap();
    if target.starts_with("/v1/markets/") {
        let mut market: Value =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
        market["market_id"] = Value::String(market_id.into());
        let mut tokens: Value =
            serde_json::from_str(market["source_specific_data"]["Tokens"].as_str().unwrap())
                .unwrap();
        tokens[1]["token_id"] = Value::String(
            market_id
                .trim_start_matches("workload-")
                .trim_end_matches("_NO")
                .into(),
        );
        market["source_specific_data"]["Tokens"] = Value::String(tokens.to_string());
        return market.to_string();
    }

    if target.starts_with("/v1/orderbook/") {
        let mut book: Value =
            serde_json::from_str(include_str!("../../test_data/poll-book.json")).unwrap();
        book["market_id"] = Value::String(market_id.into());
        return book.to_string();
    }

    if target.starts_with("/v1/ohlcv/") {
        return include_str!("../../test_data/client-bars.json").into();
    }

    assert!(target.starts_with("/v1/activity/"));
    let mut activity: Value =
        serde_json::from_str(include_str!("../../test_data/poll-activity.json")).unwrap();

    for trade in activity["trades"].as_array_mut().unwrap() {
        trade["market_id"] = Value::String(market_id.into());
    }

    activity.to_string()
}
