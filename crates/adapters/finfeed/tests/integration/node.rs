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

//! LiveNode routing, managed books, and DataTester lifecycle.

#![cfg(not(all(feature = "simulation", madsim)))]

use std::{num::NonZeroUsize, time::Duration};

use nautilus_common::{
    clients::DataClient,
    enums::Environment,
    messages::{
        DataEvent,
        data::{SubscribeBookDepth, UnsubscribeBookDepth},
    },
};
use nautilus_core::{
    UUID4, UnixNanos, string::secret::SecretString, time::get_atomic_clock_realtime,
};
use nautilus_finfeed::{
    common::{consts::FINFEED_CLIENT_ID, symbol::to_instrument_id},
    config::FinFeedDataClientConfig,
    data::FinFeedDataClient,
    factories::FinFeedDataClientFactory,
    http::client::FinFeedHttpConfig,
    instruments::FinFeedInstrumentSelection,
};
use nautilus_live::node::{LiveNode, NodeState};
use nautilus_model::{
    data::Data,
    enums::{BookType, RecordFlag},
    identifiers::{ClientId, TraderId},
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use nautilus_network::{
    dst::{task, time},
    retry::RetryConfig,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};
use tokio::io::AsyncWriteExt;

use super::dst::{assert_request, bind_peer, read_request};

const MARKET_ID: &str = "TEST-OUTCOME_NO";

#[tokio::test]
async fn data_tester_routes_quotes_and_managed_books_and_stops() {
    let (listener, address) = bind_peer().await;
    let attempts = 4;
    let (stopped, shutdown) = tokio::sync::oneshot::channel();

    let peer = task::spawn(async move {
        let mut requests = Vec::new();

        for _ in 0..attempts {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let target = request.split_whitespace().nth(1).unwrap();

            let body = if target == "/v1/exchanges" {
                include_str!("../../test_data/exchanges.json")
            } else if target == format!("/v1/markets/POLYMARKET/{MARKET_ID}") {
                include_str!("../../test_data/polymarket-market.json")
            } else if target == format!("/v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=3") {
                include_str!("../../test_data/poll-activity.json")
            } else if target == format!("/v1/orderbook/POLYMARKET/{MARKET_ID}/current") {
                include_str!("../../test_data/poll-book.json")
            } else {
                panic!("unexpected node request {target}");
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
            requests.push(request);
        }

        shutdown.await.unwrap();
        time::timeout(Duration::from_millis(200), listener.accept())
            .await
            .unwrap_err();
        requests
    });

    let http = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        attempt_budget: attempts as u64,
        ..Default::default()
    };

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http: http.clone(),
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![MARKET_ID.into()],
        }],
        connect_timeout_secs: 5,
        poll_interval_ms: 60_000,
        poll_limit: 3,
    };

    let instrument_id = to_instrument_id("POLYMARKET", MARKET_ID).unwrap();
    let mut node = data_tester_node(config);
    let cache = node.kernel().cache();
    let handle = node.handle();

    let observe = async {
        let result = time::timeout(Duration::from_secs(5), async {
            loop {
                {
                    let cache = cache.borrow();
                    if cache.quote_count(&instrument_id) == 2
                        && cache.order_book(&instrument_id).is_some_and(|book| {
                            book.bids_as_map(None).len() == 3 && book.asks_as_map(None).len() == 2
                        })
                    {
                        break;
                    }
                }

                time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;

        handle.stop();
        result
    };

    let (run, observed) = tokio::join!(node.run(), observe);
    run.unwrap();
    observed.unwrap();
    assert_eq!(handle.state(), NodeState::Stopped);
    stopped.send(()).unwrap();
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    let cache = cache.borrow();
    let quote = cache.quote(&instrument_id).unwrap();
    let book = cache.order_book(&instrument_id).unwrap();

    assert_eq!(handle.state(), NodeState::Stopped);
    assert_eq!(cache.quote_count(&instrument_id), 2);
    assert_eq!(cache.trade_count(&instrument_id), 0);
    assert_eq!(quote.instrument_id, instrument_id);
    assert_eq!(quote.bid_price.as_decimal(), "0.31".parse().unwrap());
    assert_eq!(quote.ask_price.as_decimal(), "0.41".parse().unwrap());
    assert_eq!(quote.bid_size.as_decimal(), "7.000007".parse().unwrap());
    assert_eq!(quote.ask_size.as_decimal(), "8.000008".parse().unwrap());
    assert_eq!(quote.ts_event, UnixNanos::from(1_791_676_801_000_000_000));
    assert_eq!(quote.ts_init, UnixNanos::from(1_791_676_802_000_000_000));
    assert_eq!(book.instrument_id, instrument_id);
    assert_eq!(book.book_type, BookType::L2_MBP);
    assert_eq!(book.sequence, 0);
    assert_eq!(book.update_count, 6);
    assert_eq!(book.ts_last, UnixNanos::from(1_791_676_801_000_000_000));
    assert_eq!(
        book.bids_as_map(None).into_iter().collect::<Vec<_>>(),
        vec![
            ("0.3".parse().unwrap(), "1.000001".parse().unwrap()),
            ("0.2".parse().unwrap(), "2.000002".parse().unwrap()),
            ("0.1".parse().unwrap(), "3.000003".parse().unwrap()),
        ]
    );
    assert_eq!(
        book.asks_as_map(None).into_iter().collect::<Vec<_>>(),
        vec![
            ("0.4".parse().unwrap(), "4.000004".parse().unwrap()),
            ("0.5".parse().unwrap(), "5.000005".parse().unwrap()),
        ]
    );
    assert_eq!(requests.len(), attempts);

    for request in requests {
        let target = request.split_whitespace().nth(1).unwrap();
        assert_request(&request, target, &http);
    }
}

#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most four managed GETs"]
async fn bounded_live_data_tester_publishes_quotes_and_book_and_stops() {
    let config = live_config();
    let instrument_id = to_instrument_id("POLYMARKET", MARKET_ID).unwrap();
    let mut node = data_tester_node(config);
    let cache = node.kernel().cache();
    let handle = node.handle();

    let observe = async {
        let result = time::timeout(Duration::from_secs(65), async {
            loop {
                {
                    let cache = cache.borrow();
                    if cache.quote_count(&instrument_id) > 0
                        && cache.order_book(&instrument_id).is_some_and(|book| {
                            book.best_bid_price().is_some() && book.best_ask_price().is_some()
                        })
                    {
                        break;
                    }
                }

                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;

        handle.stop();
        result
    };

    let (run, observed) = tokio::join!(node.run(), observe);
    run.unwrap();
    observed.unwrap();
    let quotes = cache.borrow().quotes(&instrument_id).unwrap();
    let book = cache.borrow().order_book(&instrument_id).unwrap().clone();
    time::sleep(Duration::from_millis(350)).await;
    let now = get_atomic_clock_realtime().get_time_ns();

    assert_eq!(handle.state(), NodeState::Stopped);
    assert_eq!(cache.borrow().quotes(&instrument_id).unwrap(), quotes);
    assert_eq!(cache.borrow().order_book(&instrument_id).unwrap(), &book);
    assert_eq!(book.instrument_id, instrument_id);
    assert_eq!(book.book_type, BookType::L2_MBP);
    assert_eq!(book.sequence, 0);
    assert!(book.ts_last > UnixNanos::default() && book.ts_last <= now);
    book_check_integrity(&book).unwrap();

    for quote in &quotes {
        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price.precision, 4);
        assert_eq!(quote.ask_price.precision, 4);
        assert_eq!(quote.bid_size.precision, 6);
        assert_eq!(quote.ask_size.precision, 6);
        assert!(quote.ts_event > UnixNanos::default() && quote.ts_event <= now);
        assert!(quote.ts_init > UnixNanos::default() && quote.ts_init <= now);
    }

    println!(
        "Rust LiveNode/DataTester published {} exact-precision quotes and a checked L2 book; node stopped with stable cached data, at most four managed GETs",
        quotes.len()
    );
}

#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most four managed GETs"]
async fn bounded_live_book_unsubscribe_resubscribe() {
    let instrument_id = to_instrument_id("POLYMARKET", MARKET_ID).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), live_config(), sender.into()).unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    let depth_limit = 5;

    for _ in 0..2 {
        client
            .subscribe_book_depth(SubscribeBookDepth::new(
                instrument_id,
                BookType::L2_MBP,
                Some(client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                NonZeroUsize::new(depth_limit),
                false,
                None,
                None,
            ))
            .unwrap();
        let event = time::timeout(Duration::from_secs(35), receiver.recv())
            .await
            .unwrap()
            .unwrap();

        let DataEvent::Data(Data::BookDepth(depth)) = event else {
            panic!("expected book depth after each subscription");
        };

        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        book.apply_depth(&depth).unwrap();
        book_check_integrity(&book).unwrap();
        assert_eq!(depth.instrument_id, instrument_id);
        assert_eq!(depth.sequence, 0);
        assert_eq!(
            depth.flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
        assert_eq!(depth.bid_counts.as_slice(), vec![0; depth.bids.len()]);
        assert_eq!(depth.ask_counts.as_slice(), vec![0; depth.asks.len()]);
        assert!(depth.ts_init >= depth.ts_event);
        assert!(depth.ts_init <= get_atomic_clock_realtime().get_time_ns());
        assert_eq!(depth.bids.len(), depth_limit);
        assert_eq!(depth.asks.len(), depth_limit);
        assert!(depth.ts_event > UnixNanos::default());
        assert!(depth.ts_event <= get_atomic_clock_realtime().get_time_ns());
        client
            .unsubscribe_book_depth(&UnsubscribeBookDepth::new(
                instrument_id,
                Some(client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        assert!(
            time::timeout(Duration::from_millis(350), receiver.recv())
                .await
                .is_err()
        );
    }

    client.disconnect().await.unwrap();
    assert!(
        time::timeout(Duration::from_millis(350), receiver.recv())
            .await
            .is_err()
    );
    println!(
        "Live book unsubscribe/resubscribe delivered a checked depth{depth_limit} snapshot for both subscriptions and stayed quiet after each unsubscribe and shutdown; at most four managed GETs"
    );
}

fn data_tester_node(config: FinFeedDataClientConfig) -> LiveNode {
    let instrument_id = to_instrument_id("POLYMARKET", MARKET_ID).unwrap();
    let timeout = config.connect_timeout_secs;
    let mut node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Live)
        .unwrap()
        .with_name("FinFeedDataTester")
        .with_load_state(false)
        .with_save_state(false)
        .with_reconciliation(false)
        .with_timeout_connection(timeout)
        .with_timeout_portfolio(0)
        .with_delay_post_stop_secs(0)
        .with_delay_shutdown_secs(0)
        .add_data_client(
            None,
            Box::new(FinFeedDataClientFactory::new()),
            Box::new(config),
        )
        .unwrap()
        .build()
        .unwrap();
    let tester = DataTesterConfig::builder()
        .client_id(*FINFEED_CLIENT_ID)
        .instrument_ids(vec![instrument_id])
        .subscribe_quotes(true)
        .subscribe_book_deltas(true)
        .stats_interval_secs(0)
        .build()
        .unwrap();
    node.add_actor(DataTester::new(tester)).unwrap();
    node
}

fn live_config() -> FinFeedDataClientConfig {
    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");

    FinFeedDataClientConfig {
        api_key: Some(SecretString::from(key)),
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![MARKET_ID.into()],
        }],
        http: FinFeedHttpConfig {
            attempt_budget: 4,
            retry: RetryConfig {
                max_retries: 0,
                max_elapsed_ms: Some(30_000),
                ..Default::default()
            },
            ..Default::default()
        },
        connect_timeout_secs: 60,
        poll_interval_ms: 60_000,
        ..Default::default()
    }
}
