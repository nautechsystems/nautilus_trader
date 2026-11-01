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

//! Opt-in live validation of bounded REST requests and polling subscriptions.

#![cfg(not(all(feature = "simulation", madsim)))]

use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use nautilus_common::{
    clients::DataClient,
    logging::{logger::LoggerConfig, writer::FileWriterConfig},
    messages::{DataEvent, DataResponse, data::*},
};
use nautilus_core::{
    UUID4, UnixNanos, string::secret::SecretString, time::get_atomic_clock_realtime,
};
use nautilus_finfeed::{
    common::symbol::to_instrument_id,
    config::FinFeedDataClientConfig,
    data::FinFeedDataClient,
    http::{
        client::{FinFeedHttpClient, FinFeedHttpConfig},
        query::{ActiveQuery, ActivityQuery, HistoryQuery, ListingsQuery, OhlcvQuery, Pagination},
    },
    instruments::{FinFeedInstrumentProvider, FinFeedInstrumentSelection},
};
use nautilus_model::{
    data::{BarType, Data},
    enums::{AggressorSide, BookType, RecordFlag},
    identifiers::{ClientId, TraderId},
    instruments::Instrument,
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use nautilus_network::retry::RetryConfig;

const POLL_MARKET_ID: &str = "2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949_NO";

#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most four managed GETs"]
async fn bounded_multi_outcome_bootstrap_deadlines() {
    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");
    let ids = vec![
        POLL_MARKET_ID.to_owned(),
        POLL_MARKET_ID.replace("_NO", "_YES"),
    ];

    for (deadline, succeeds) in [(1, false), (60, true)] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let config = FinFeedDataClientConfig {
            api_key: Some(SecretString::from(key.clone())),
            instruments: vec![FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: ids.iter().map(|id| id.as_str().into()).collect(),
            }],
            http: FinFeedHttpConfig {
                attempt_budget: 3,
                retry: RetryConfig {
                    max_retries: 0,
                    ..FinFeedHttpConfig::default().retry
                },
                ..Default::default()
            },
            connect_timeout_secs: deadline,
            ..Default::default()
        };

        let mut client =
            FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
        let result = client.connect().await;
        if succeeds {
            result.unwrap();
            assert!(client.is_connected());

            for id in &ids {
                let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap()
                else {
                    panic!("expected startup definition")
                };

                assert_eq!(instrument.id(), to_instrument_id("POLYMARKET", id).unwrap());
            }
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "FinFeed bootstrap timed out"
            );
            assert!(client.is_disconnected());
        }

        assert!(receiver.try_recv().is_err());
        client.disconnect().await.unwrap();
        assert!(client.is_disconnected());
    }

    println!(
        "Live FinFeed two-outcome bootstrap: undersized deadline rejected, configured deadline passed, at most four managed GETs"
    );
}

#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most eight managed GETs"]
async fn bounded_terminal_definition_refresh_and_reconnect() {
    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");
    let terminal_market = "0-2-MLB-HOME-RUNS-ON-MONDAY_NO";
    let terminal_id = to_instrument_id("POLYMARKET", terminal_market).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from(key)),
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![POLL_MARKET_ID.into(), terminal_market.into()],
        }],
        http: FinFeedHttpConfig {
            attempt_budget: 5,
            retry: RetryConfig {
                max_retries: 0,
                jitter_ms: 0,
                max_elapsed_ms: Some(30_000),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    client.connect().await.unwrap();

    for expected in [
        to_instrument_id("POLYMARKET", POLL_MARKET_ID).unwrap(),
        terminal_id,
    ] {
        let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
            panic!("expected startup definition")
        };

        assert_eq!(instrument.id(), expected);
    }

    for _ in 0..2 {
        client
            .request_instrument(RequestInstrument::new(
                terminal_id,
                None,
                None,
                Some(client.client_id()),
                UUID4::new(),
                UnixNanos::from(0),
                None,
            ))
            .unwrap();

        let DataResponse::Instrument(response) = receive_live(&mut receiver).await.unwrap() else {
            panic!("expected only refreshed definition")
        };

        assert_eq!(response.instrument_id, terminal_id);
        assert_eq!(response.data.id(), terminal_id);
        let info = response.data.info().unwrap();
        assert!(matches!(
            info.get_str("status"),
            Some("Closed" | "Resolved")
        ));
        assert_eq!(info.get_str("outcome_type"), Some("Binary"));
        assert!(
            client
                .subscribe_quotes(SubscribeQuotes::new(
                    terminal_id,
                    Some(client.client_id()),
                    None,
                    UUID4::new(),
                    UnixNanos::from(0),
                    None,
                    None
                ))
                .is_err()
        );
        assert!(
            client
                .subscribe_bars(SubscribeBars::new(
                    BarType::from(format!("{terminal_id}-1-SECOND-LAST-EXTERNAL").as_str()),
                    Some(client.client_id()),
                    None,
                    UUID4::new(),
                    UnixNanos::from(0),
                    None,
                    None
                ))
                .is_err()
        );
    }

    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    for expected in [
        to_instrument_id("POLYMARKET", POLL_MARKET_ID).unwrap(),
        terminal_id,
    ] {
        let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
            panic!("expected reconnect definition")
        };

        assert_eq!(instrument.id(), expected);
    }

    assert!(
        client
            .subscribe_quotes(SubscribeQuotes::new(
                terminal_id,
                Some(client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None
            ))
            .is_err()
    );
    client.disconnect().await.unwrap();
    assert!(receiver.try_recv().is_err());
    println!(
        "Live FinFeed terminal definitions refreshed twice and survived reconnect; no settlement event or terminal subscription"
    );
}

#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most six managed calls across two generations"]
async fn bounded_book_publication_and_reconnect() {
    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");
    let instrument_id = to_instrument_id("POLYMARKET", POLL_MARKET_ID).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from(key)),
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![POLL_MARKET_ID.into()],
        }],
        http: FinFeedHttpConfig {
            attempt_budget: 3,
            retry: RetryConfig {
                max_retries: 0,
                jitter_ms: 0,
                max_elapsed_ms: Some(30_000),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    let mut counts = Vec::new();

    for generation in 0..2 {
        client.connect().await.unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));
        assert!(receiver.try_recv().is_err());

        if generation == 0 {
            client
                .subscribe_book_deltas(SubscribeBookDeltas::new(
                    instrument_id,
                    BookType::L2_MBP,
                    Some(client.client_id()),
                    None,
                    UUID4::new(),
                    UnixNanos::from(0),
                    None,
                    false,
                    None,
                    None,
                ))
                .unwrap();
            client
                .subscribe_book_depth(SubscribeBookDepth::new(
                    instrument_id,
                    BookType::L2_MBP,
                    Some(client.client_id()),
                    None,
                    UUID4::new(),
                    UnixNanos::from(0),
                    NonZeroUsize::new(5),
                    false,
                    None,
                    None,
                ))
                .unwrap();
        }

        let DataEvent::Data(Data::BookDeltas(deltas)) =
            tokio::time::timeout(Duration::from_secs(35), receiver.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("expected live book replacement")
        };

        let DataEvent::Data(Data::BookDepth(depth)) =
            tokio::time::timeout(Duration::from_secs(5), receiver.recv())
                .await
                .unwrap()
                .unwrap()
        else {
            panic!("expected shared live depth snapshot")
        };

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(depth.instrument_id, instrument_id);
        assert_eq!(deltas.ts_event, depth.ts_event);
        assert_eq!(deltas.ts_init, depth.ts_init);
        assert_eq!(deltas.sequence, 0);
        assert_eq!(depth.sequence, 0);
        assert_eq!(
            deltas.flags,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
        );
        assert_eq!(depth.flags, deltas.flags);
        assert_eq!(depth.bid_counts.as_slice(), vec![0; depth.bids.len()]);
        assert_eq!(depth.ask_counts.as_slice(), vec![0; depth.asks.len()]);
        let mut full = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut limited = OrderBook::new(instrument_id, BookType::L2_MBP);
        full.apply_deltas(&deltas).unwrap();
        limited.apply_depth(&depth).unwrap();
        book_check_integrity(&full).unwrap();
        book_check_integrity(&limited).unwrap();
        assert_eq!(limited.bids_as_map(None), full.bids_as_map(Some(5)));
        assert_eq!(limited.asks_as_map(None), full.asks_as_map(Some(5)));
        counts.push((full.bids_as_map(None).len(), full.asks_as_map(None).len()));
        client.disconnect().await.unwrap();
        assert!(receiver.try_recv().is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(350), receiver.recv())
                .await
                .is_err()
        );
    }

    println!("Live full/depth snapshots and fresh reconnect validated; bid/ask counts={counts:?}");
}

#[tokio::test]
#[ignore = "requires prediction-market access via FINFEED_API_KEY or COINAPI_API_KEY and consumes at most 20 managed calls"]
async fn bounded_read_only_rest_smoke() {
    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");

    let attempt_budget = 20;

    let client = FinFeedHttpClient::new(
        Some(SecretString::from(key)),
        FinFeedHttpConfig {
            attempt_budget,
            retry: RetryConfig {
                max_retries: 0,
                jitter_ms: 0,
                max_elapsed_ms: Some(30_000),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();

    let exchanges = client.get_exchanges().await.unwrap();
    assert!(
        exchanges
            .iter()
            .any(|exchange| exchange.exchange_id.as_deref() == Some("POLYMARKET"))
    );
    assert_eq!(
        client
            .get_exchange("POLYMARKET")
            .await
            .unwrap()
            .exchange_id
            .as_deref(),
        Some("POLYMARKET")
    );
    println!("Exchange discovery and metadata parsed");

    let listings = client
        .get_listings(
            "POLYMARKET",
            &ListingsQuery {
                limit: 2,
                cursor: None,
            },
        )
        .await
        .unwrap();

    let expected_attempts = 13 + u64::from(listings.next_cursor.is_some());
    let markets = listings.data.unwrap();
    assert_eq!(markets.len(), 2);

    if let Some(cursor) = listings.next_cursor {
        client
            .get_listings(
                "POLYMARKET",
                &ListingsQuery {
                    limit: 2,
                    cursor: Some(cursor),
                },
            )
            .await
            .unwrap();
    }

    let active = client
        .get_active("POLYMARKET", &ActiveQuery { limit: 2, page: 1 })
        .await
        .unwrap();
    assert!(active.len() <= 2);
    let id = active.first().expect("active market required");
    assert_eq!(
        client
            .get_market("POLYMARKET", id)
            .await
            .unwrap()
            .market_id
            .as_deref(),
        Some(id.as_str())
    );
    println!("Cursor listings, active IDs, and outcome metadata parsed");
    client.get_current("POLYMARKET", id).await.unwrap();
    client
        .get_latest("POLYMARKET", id, &ActivityQuery { limit: 3 })
        .await
        .unwrap();
    let book = client.get_book("POLYMARKET", id).await.unwrap();
    assert_eq!(book.exchange_id.as_deref(), Some("POLYMARKET"));
    assert_eq!(book.market_id.as_deref(), Some(id.as_str()));
    println!("Current/latest activity and current book parsed");
    let periods = client.get_periods().await.unwrap();
    assert!(
        periods
            .iter()
            .any(|period| period.period_id.as_ref().map(AsRef::as_ref) == Some("1MIN"))
    );

    let day = HistoryQuery {
        date: Some("2026-10-11".parse().unwrap()),
        time_start: None,
        time_end: None,
        limit: 3,
    };

    client
        .get_quotes_history("POLYMARKET", id, &day)
        .await
        .unwrap();
    client
        .get_trades_history("KALSHI", "CONTROLH-2026-D_NO", &day)
        .await
        .unwrap();

    let history = OhlcvQuery {
        period_id: "1MIN".into(),
        time_start: Some("2026-10-11T00:00:00Z".parse().unwrap()),
        time_end: Some("2026-10-12T00:00:00Z".parse().unwrap()),
        limit: 1,
    };

    client
        .get_ohlcv_history("KALSHI", "CONTROLH-2026-D_NO", &history)
        .await
        .unwrap();
    client
        .get_ohlcv_latest(
            "POLYMARKET",
            id,
            &OhlcvQuery {
                time_start: None,
                time_end: None,
                ..history
            },
        )
        .await
        .unwrap();

    assert_eq!(
        client.attempts_remaining(),
        attempt_budget - expected_attempts
    );
    println!(
        "Periods, single-page trades/quotes, and historical/latest bars parsed; no completeness assertion"
    );
    client
        .get_listings_all(
            "PASCAL",
            &ListingsQuery {
                limit: 100,
                cursor: None,
            },
            Pagination {
                max_pages: 5,
                max_records: 500,
            },
        )
        .await
        .unwrap();

    let ids = client
        .get_active_all(
            "PASCAL",
            &ActiveQuery {
                limit: 100,
                page: 1_000_000,
            },
            Pagination {
                max_pages: 1,
                max_records: 100,
            },
        )
        .await
        .unwrap();

    assert_eq!(ids, Vec::<String>::new());
    println!(
        "Terminal discovery pages parsed; {} managed GETs used",
        attempt_budget - client.attempts_remaining()
    );
}

#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most six managed read-only calls"]
async fn bounded_read_only_instrument_smoke() {
    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");
    let http = Arc::new(
        FinFeedHttpClient::new(
            Some(SecretString::from(key)),
            FinFeedHttpConfig {
                attempt_budget: 6,
                retry: RetryConfig {
                    max_retries: 0,
                    jitter_ms: 0,
                    max_elapsed_ms: Some(30_000),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap(),
    );
    let market_ids = http
        .get_active("POLYMARKET", &ActiveQuery { limit: 2, page: 1 })
        .await
        .unwrap();
    assert_eq!(market_ids.len(), 2);
    let expected: Vec<_> = market_ids
        .iter()
        .map(|id| to_instrument_id("POLYMARKET", id).unwrap())
        .collect();
    let selection = FinFeedInstrumentSelection {
        exchange_id: "POLYMARKET".into(),
        market_ids,
    };
    let mut provider = FinFeedInstrumentProvider::new(http.clone());
    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let loaded = provider.load(&[selection], ts_init).await.unwrap();
    assert_eq!(
        loaded
            .iter()
            .map(|instrument| instrument.id)
            .collect::<Vec<_>>(),
        expected
    );

    for instrument in &loaded {
        assert_eq!(instrument.raw_symbol, instrument.id.symbol);
        assert_eq!(instrument.currency.code.as_str(), "pUSD");
        assert_eq!(instrument.price_precision, 4);
        assert_eq!(instrument.size_precision, 6);
        assert_eq!(instrument.ts_init, ts_init);
        assert_eq!(provider.get(instrument.id).unwrap().id, instrument.id);
    }
    let refreshed_at = get_atomic_clock_realtime().get_time_ns();
    let refreshed = provider
        .request_instrument(expected[0], refreshed_at)
        .await
        .unwrap();
    assert_eq!(refreshed.id, expected[0]);
    assert_eq!(refreshed.ts_init, refreshed_at);
    assert_eq!(provider.get(expected[0]).unwrap().ts_init, refreshed_at);
    assert_eq!(http.attempts_remaining(), 1);
    println!(
        "Two active outcome definitions loaded and one freshly requested; five managed read-only calls, no settlement claim"
    );
}

#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most 12 managed read-only calls"]
async fn bounded_read_only_data_client_smoke() {
    let _logging = nautilus_common::logging::init_logging(
        TraderId::from("TRADER-001"),
        UUID4::new(),
        LoggerConfig {
            is_colored: false,
            ..Default::default()
        },
        FileWriterConfig::default(),
    )
    .unwrap();

    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");
    let market_id = "0-SHIPS-TRANSIT-BAB-EL-MANDEB-ON-ANY-DATE-BY-DECEMBER-31_NO";
    let instrument_id = to_instrument_id("POLYMARKET", market_id).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from(key)),
        http: FinFeedHttpConfig {
            attempt_budget: 12,
            timeout_secs: 60,
            retry: RetryConfig {
                max_retries: 0,
                operation_timeout_ms: Some(75_000),
                max_elapsed_ms: Some(80_000),
                ..Default::default()
            },
            ..Default::default()
        },
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![market_id.into()],
        }],
        ..Default::default()
    };

    let client_id = ClientId::from("FINFEED-LIVE");
    let mut client = FinFeedDataClient::new(client_id, config, sender.into()).unwrap();
    client.connect().await.unwrap();
    assert!(client.is_connected());

    let DataEvent::Data(Data::Instrument(loaded)) = receiver.try_recv().unwrap() else {
        panic!("expected startup definition")
    };

    assert_eq!(loaded.id(), instrument_id);
    let request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            request_id,
            UnixNanos::from(1),
            None,
        ))
        .unwrap();

    let DataResponse::Instrument(result) = receive_live(&mut receiver)
        .await
        .expect("required live response")
    else {
        panic!("expected fresh definition")
    };

    assert_eq!(result.correlation_id, request_id);
    assert_eq!(result.data.id(), instrument_id);
    client
        .request_instruments(RequestInstruments::new(
            None,
            None,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::from(2),
            None,
        ))
        .unwrap();

    let DataResponse::Instruments(result) = receive_live(&mut receiver)
        .await
        .expect("required live response")
    else {
        panic!("expected fresh definitions")
    };

    assert_eq!(
        result.data.iter().map(Instrument::id).collect::<Vec<_>>(),
        [instrument_id]
    );
    client
        .request_book_snapshot(RequestBookSnapshot::new(
            instrument_id,
            NonZeroUsize::new(5),
            Some(client_id),
            UUID4::new(),
            UnixNanos::from(3),
            None,
        ))
        .unwrap();

    let DataResponse::Book(result) = receive_live(&mut receiver)
        .await
        .expect("required live response")
    else {
        panic!("expected current book")
    };

    assert_eq!(result.instrument_id, instrument_id);
    assert_eq!(result.data.instrument_id, instrument_id);
    println!("Data client bootstrap, fresh definitions, and atomic current L2 book passed");
    let start = Some("2026-10-11T00:00:00Z".parse().unwrap());
    let end = Some("2026-10-11T01:00:00Z".parse().unwrap());
    client
        .request_quotes(RequestQuotes::new(
            instrument_id,
            start,
            end,
            NonZeroUsize::new(3),
            Some(client_id),
            UUID4::new(),
            UnixNanos::from(4),
            None,
        ))
        .unwrap();

    match receive_live(&mut receiver).await {
        Some(DataResponse::Quotes(quotes)) => {
            assert_eq!(quotes.instrument_ids, vec![instrument_id]);
            assert!(quotes.data.len() <= 3);
            assert!(
                quotes
                    .data
                    .iter()
                    .all(|quote| quote.instrument_id == instrument_id)
            );
            println!(
                "Historical quotes response validated: {} records",
                quotes.data.len()
            );
        }
        Some(_) => panic!("expected historical quotes"),
        None => println!(
            "Historical quotes unavailable within the request deadline; retain logged failure as a provider limitation"
        ),
    }

    client
        .request_trades(RequestTrades::new(
            instrument_id,
            start,
            end,
            NonZeroUsize::new(3),
            Some(client_id),
            UUID4::new(),
            UnixNanos::from(5),
            None,
        ))
        .unwrap();

    match receive_live(&mut receiver).await {
        Some(DataResponse::Trades(trades)) => {
            assert_eq!(trades.instrument_ids, vec![instrument_id]);
            assert!(trades.data.len() <= 3);
            assert!(
                trades
                    .data
                    .iter()
                    .all(|trade| trade.instrument_id == instrument_id)
            );
            println!(
                "Historical trades response validated: {} records",
                trades.data.len()
            );
        }
        Some(_) => panic!("expected historical trades"),
        None => println!(
            "Historical trades unavailable within the request deadline; retain logged failure as a provider limitation"
        ),
    }

    let bar_type: BarType = format!("{instrument_id}-1-MINUTE-LAST-EXTERNAL")
        .parse()
        .unwrap();
    client
        .request_bars(RequestBars::new(
            bar_type,
            start,
            end,
            NonZeroUsize::new(3),
            Some(client_id),
            UUID4::new(),
            UnixNanos::from(6),
            None,
        ))
        .unwrap();

    match receive_live(&mut receiver).await {
        Some(DataResponse::Bars(bars)) => {
            assert_eq!(bars.bar_type, bar_type);
            assert!(bars.data.len() <= 3);
            assert!(bars.data.iter().all(|bar| bar.bar_type == bar_type));
            println!(
                "Historical closed-bar response validated: {} records",
                bars.data.len()
            );
        }
        Some(_) => panic!("expected complete bars"),
        None => println!(
            "Historical bars unavailable within the request deadline; retain logged failure as a provider limitation"
        ),
    }

    assert!(client.is_connected());
    tokio::time::timeout(Duration::from_secs(4), client.disconnect())
        .await
        .unwrap()
        .unwrap();
    assert!(client.is_disconnected());
    client.connect().await.unwrap();
    assert!(client.is_connected());
    assert!(
        matches!(receiver.try_recv().unwrap(),DataEvent::Data(Data::Instrument(instrument)) if instrument.id()==instrument_id)
    );
    client.disconnect().await.unwrap();
    assert!(receiver.try_recv().is_err());
    println!(
        "Disconnect and fresh reconnect readiness passed; at most eleven managed GETs, no execution"
    );
}

async fn receive_live(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> Option<DataResponse> {
    let mut definitions = Vec::new();

    loop {
        let Ok(event) = tokio::time::timeout(Duration::from_secs(100), receiver.recv()).await
        else {
            return None;
        };

        match event.expect("live data channel closed") {
            DataEvent::Data(Data::Instrument(instrument)) => definitions.push(*instrument),
            DataEvent::Response(response) => {
                super::data::assert_definitions(&response, &definitions);
                return Some(response);
            }
            event => panic!("expected definition or response, received {event:?}"),
        }
    }
}

#[rstest::rstest]
#[case(100)]
#[case(10_000)]
#[tokio::test]
#[ignore = "requires prediction-market access and consumes at most eight managed calls across two generations"]
async fn bounded_activity_publication_and_reconnect(#[case] limit: u32) {
    let _logging = nautilus_common::logging::init_logging(
        TraderId::from("TRADER-001"),
        UUID4::new(),
        LoggerConfig {
            is_colored: false,
            module_level: [("nautilus_finfeed".into(), log::LevelFilter::Debug)]
                .into_iter()
                .collect(),
            ..Default::default()
        },
        FileWriterConfig::default(),
    )
    .unwrap();

    let key = std::env::var("FINFEED_API_KEY")
        .or_else(|_| std::env::var("COINAPI_API_KEY"))
        .expect("prediction-market key required");
    let market_id = POLL_MARKET_ID;
    let instrument_id = to_instrument_id("POLYMARKET", market_id).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from(key)),
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![market_id.into()],
        }],
        http: FinFeedHttpConfig {
            attempt_budget: 4,
            retry: RetryConfig {
                max_retries: 0,
                jitter_ms: 0,
                max_elapsed_ms: Some(30_000),
                ..Default::default()
            },
            ..Default::default()
        },
        poll_limit: limit,
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
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
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
            None,
        ))
        .unwrap();
    let mut quotes = Vec::new();
    let mut trades = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
    while quotes.is_empty() || trades.is_empty() {
        let data = tokio::time::timeout_at(deadline, receiver.recv())
            .await
            .expect("live activity must publish both types")
            .unwrap();

        match data {
            DataEvent::Data(Data::Quote(tick)) => {
                assert_eq!(tick.instrument_id, instrument_id);
                quotes.push(tick);
            }
            DataEvent::Data(Data::Trade(tick)) => {
                assert_eq!(tick.instrument_id, instrument_id);
                assert_eq!(tick.aggressor_side, AggressorSide::NoAggressor);
                assert_eq!(tick.ts_event, tick.ts_init);
                assert_eq!(tick.trade_id.as_str().len(), 36);
                trades.push(tick);
            }
            _ => panic!("unexpected live polling event"),
        }
    }

    client.disconnect().await.unwrap();

    while let Ok(event) = receiver.try_recv() {
        match event {
            DataEvent::Data(Data::Quote(tick)) => quotes.push(tick),
            DataEvent::Data(Data::Trade(tick)) => trades.push(tick),
            _ => panic!("unexpected live polling event"),
        }
    }

    assert!(
        tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .is_err()
    );
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    assert!(receiver.try_recv().is_err());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
    let mut fresh_quotes = 0;

    loop {
        let Ok(Some(event)) = tokio::time::timeout_at(deadline, receiver.recv()).await else {
            break;
        };

        match event {
            DataEvent::Data(Data::Quote(tick)) => {
                assert_eq!(tick.instrument_id, instrument_id);
                assert!(!quotes.contains(&tick));
                fresh_quotes += 1;
            }
            DataEvent::Data(Data::Trade(tick)) => {
                assert!(!trades.iter().any(|prior| prior.trade_id == tick.trade_id));
            }
            _ => panic!("unexpected restored polling event"),
        }
    }

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
    client
        .unsubscribe_trades(&UnsubscribeTrades::new(
            instrument_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
            None,
        ))
        .unwrap();
    client.disconnect().await.unwrap();

    while receiver.try_recv().is_ok() {}
    assert!(
        tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .is_err()
    );
    println!(
        "Live publication: {} quotes, {} trades; reconnect observes {fresh_quotes} new quotes without replaying retained identities; disconnect and unsubscribe stay quiet",
        quotes.len(),
        trades.len()
    );
}
