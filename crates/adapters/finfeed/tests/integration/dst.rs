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

//! REST transport, discovery, and instrument definitions under deterministic simulation.

use std::{sync::Arc, time::Duration};

use nautilus_core::{UnixNanos, string::secret::SecretString};
use nautilus_finfeed::{
    common::symbol::to_instrument_id,
    config::FinFeedDataClientConfig,
    data::FinFeedDataClient,
    http::{
        client::{FinFeedHttpClient, FinFeedHttpConfig},
        error::Error,
        models::*,
        parse::parse_instrument,
        query::*,
    },
    instruments::{FinFeedInstrumentProvider, FinFeedInstrumentSelection},
};
use nautilus_model::identifiers::ClientId;
use nautilus_network::{
    dst::{
        net::{TcpListener, TcpStream},
        task, time,
    },
    retry::{RetryConfig, RetryError},
};
use rstest::rstest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const EXCHANGES: &str = include_str!("../../test_data/exchanges.json");

pub(super) fn new_client(config: FinFeedHttpConfig) -> FinFeedHttpClient {
    FinFeedHttpClient::new(Some(SecretString::from("test-api-key")), config).unwrap()
}

pub(super) async fn start_peer(
    responses: Vec<(u16, &'static str, impl Into<String> + Send + 'static)>,
) -> (task::JoinHandle<Vec<String>>, FinFeedHttpConfig) {
    start_peer_delayed(responses, Duration::ZERO).await
}

pub(super) async fn start_peer_delayed(
    responses: Vec<(u16, &'static str, impl Into<String> + Send + 'static)>,
    delay: Duration,
) -> (task::JoinHandle<Vec<String>>, FinFeedHttpConfig) {
    let (listener, address) = bind_peer().await;

    let config = FinFeedHttpConfig::builder()
        .base_url(format!("http://{address}/v1/"))
        .min_request_interval_ms(1)
        .retry(RetryConfig {
            initial_delay_ms: 10,
            max_delay_ms: 100,
            jitter_ms: 0,
            max_retries: 2,
            operation_timeout_ms: Some(2_000),
            max_elapsed_ms: Some(5_000),
            ..Default::default()
        })
        .build();

    let task = task::spawn(async move {
        let mut requests = Vec::new();

        for (status, headers, body) in responses {
            let body = body.into();
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut stream).await);
            time::sleep(delay).await;
            stream.write_all(format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len()).as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
        }

        requests
    });

    (task, config)
}

pub(super) fn assert_request(request: &str, target: &str, config: &FinFeedHttpConfig) {
    let mut lines = request.split("\r\n");
    assert_eq!(lines.next().unwrap(), format!("GET {target} HTTP/1.1"));
    let mut headers: Vec<_> = lines.filter(|line| !line.is_empty()).collect();
    headers.sort_unstable();
    let host = format!(
        "host: {}",
        config
            .base_url
            .expose_secret()
            .trim_start_matches("http://")
            .trim_end_matches("/v1/")
    );
    assert_eq!(
        headers,
        vec![
            "accept-encoding: gzip",
            "accept: application/json",
            "authorization: test-api-key",
            &host,
            "x-referrer: nautilus"
        ]
    );
    #[cfg(all(feature = "simulation", madsim))]
    assert_eq!(
        request,
        format!(
            "GET {target} HTTP/1.1\r\naccept: application/json\r\naccept-encoding: gzip\r\nauthorization: test-api-key\r\n{host}\r\nx-referrer: nautilus\r\n\r\n"
        )
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn exchanges_use_authenticated_shared_transport() {
    let (peer, config) = start_peer(vec![(200, "", EXCHANGES)]).await;
    let exchanges = new_client(config.clone()).get_exchanges().await.unwrap();
    let requests = time::timeout(Duration::from_secs(3), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(exchanges.len(), 9);
    assert_eq!(exchanges[0].exchange_id.as_deref(), Some("KALSHI"));
    assert_eq!(requests.len(), 1);
    assert_request(&requests[0], "/v1/exchanges", &config);
}

const MARKET: &str = include_str!("../../test_data/polymarket-market.json");
const CURRENT: &str = include_str!("../../test_data/polymarket-current.json");
const LATEST: &str = include_str!("../../test_data/polymarket-latest.json");
const BOOK: &str = include_str!("../../test_data/polymarket-book.json");
const PERIODS: &str = include_str!("../../test_data/periods.json");
const BARS: &str = include_str!("../../test_data/kalshi-ohlcv-history.json");
const TRADES: &str = include_str!("../../test_data/kalshi-trades-day.json");
const QUOTES: &str = include_str!("../../test_data/polymarket-quotes-day.json");
const LISTINGS: &str = include_str!("../../test_data/polymarket-listings.json");
const LISTINGS_NEXT: &str = include_str!("../../test_data/polymarket-listings-next.json");
const TERMINAL: &str = include_str!("../../test_data/terminal-listings.json");
const EMPTY: &str = include_str!("../../test_data/empty.json");
const ACTIVE_ONE: &str = include_str!("../../test_data/active-one.json");
const CYCLE: &str = include_str!("../../test_data/cycle-listings.json");

fn history() -> HistoryQuery {
    HistoryQuery {
        date: None,
        time_start: Some("2026-10-11T00:00:00.0000001Z".parse().unwrap()),
        time_end: Some("2026-10-12T00:00:00.0000003Z".parse().unwrap()),
        limit: 3,
    }
}

fn ohlcv() -> OhlcvQuery {
    OhlcvQuery {
        period_id: "1MIN".into(),
        time_start: history().time_start,
        time_end: history().time_end,
        limit: 1,
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unknown_metadata_labels_preserve_listing_siblings_and_checked_mapping() {
    let mut listing: serde_json::Value = serde_json::from_str(LISTINGS).unwrap();
    let known = listing["data"][0].clone();
    listing["data"].as_array_mut().unwrap().push(known);
    listing["data"][0]["status"] = "FutureStatus".into();
    listing["data"][1]["outcome_type"] = "FutureOutcome".into();
    let (peer, config) = start_peer(vec![(200, "", listing.to_string())]).await;

    let page = new_client(config.clone())
        .get_listings(
            "POLYMARKET",
            &ListingsQuery {
                limit: 3,
                cursor: None,
            },
        )
        .await
        .unwrap();

    let markets = page.data.unwrap();
    let unknown_status = parse_instrument(&markets[0], UnixNanos::from(123));
    let unknown_outcome = parse_instrument(&markets[1], UnixNanos::from(123));
    let known = parse_instrument(&markets[2], UnixNanos::from(123));

    assert_eq!(markets.len(), 3);
    assert_eq!(page.next_cursor.as_deref(), listing["next_cursor"].as_str());
    assert_eq!(markets[0].status, MarketStatus::Unknown);
    assert_eq!(markets[0].outcome_type, OutcomeType::Binary);
    assert_eq!(markets[1].status, MarketStatus::Closed);
    assert_eq!(markets[1].outcome_type, OutcomeType::Other);
    assert_eq!(markets[2].status, MarketStatus::Closed);
    assert_eq!(markets[2].outcome_type, OutcomeType::Binary);
    let instrument = unknown_status.unwrap();
    assert_eq!(instrument.info.as_ref().unwrap()["status"], "Unknown");
    assert_eq!(
        instrument.id.symbol.as_str(),
        markets[0].market_id.as_deref().unwrap()
    );
    assert_eq!(instrument.expiration_ns, known.unwrap().expiration_ns);
    assert!(matches!(unknown_outcome, Err(Error::Definition {
        exchange_id, market_id, field: "outcome_type", reason: "is unsupported"
    }) if exchange_id == "POLYMARKET" && Some(market_id.as_str()) == markets[1].market_id.as_deref()));
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request(
        &requests[0],
        "/v1/markets/POLYMARKET/history?limit=3",
        &config,
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn all_endpoint_paths_queries_and_models() {
    let exchange = include_str!("../../test_data/exchange.json");
    let (peer, config) = start_peer(vec![
        (200, "", exchange),
        (200, "", MARKET),
        (200, "", CURRENT),
        (200, "", LATEST),
        (200, "", BOOK),
        (200, "", PERIODS),
        (200, "", BARS),
        (200, "", EMPTY),
        (200, "", TRADES),
        (200, "", QUOTES),
        (200, "", TRADES),
        (200, "", include_str!("../../test_data/empty-current.json")),
    ])
    .await;
    let client = new_client(config.clone());
    let id = "opaque/NO?+=%";
    assert_eq!(
        client.get_exchange("POLYMARKET").await.unwrap(),
        Exchange {
            exchange_id: Some("POLYMARKET".into()),
            market_name_institution_description: Some("exchange description".into()),
            website: Some("https://example.com".into())
        }
    );
    let market = client.get_market("POLYMARKET", id).await.unwrap();
    assert_eq!(market.market_id.as_deref(), Some("TEST-OUTCOME_NO"));
    assert_eq!(market.exchange_id.as_deref(), Some("POLYMARKET"));
    assert_eq!(market.outcome_name.as_deref(), Some("No"));
    assert_eq!(market.status, MarketStatus::Open);
    assert_eq!(market.outcome_type, OutcomeType::Binary);
    assert_eq!(market.price, "0.42".parse().unwrap());
    assert_eq!(
        client.get_current("POLYMARKET", id).await.unwrap().trade,
        None
    );
    let latest = client
        .get_latest("POLYMARKET", id, &ActivityQuery { limit: 3 })
        .await
        .unwrap();
    assert_eq!(latest.trades, Some(vec![]));
    assert_eq!(latest.quotes.unwrap().len(), 3);
    let book = client.get_book("POLYMARKET", id).await.unwrap();
    assert_eq!(book.asks.as_ref().unwrap().len(), 10);
    assert_eq!(book.bids.as_ref().unwrap().len(), 36);
    assert_eq!(
        book.asks.as_ref().unwrap()[0],
        BookLevel {
            price: "0.6".parse().unwrap(),
            size: "41.25".parse().unwrap()
        }
    );
    assert_eq!(client.get_periods().await.unwrap().len(), 43);
    let bars = client
        .get_ohlcv_history("KALSHI", id, &ohlcv())
        .await
        .unwrap();
    assert_eq!(
        bars,
        vec![Ohlcv {
            market_id_exchange: None,
            price_open: Some("0.2".parse().unwrap()),
            price_high: Some("0.4".parse().unwrap()),
            price_low: Some("0.1".parse().unwrap()),
            price_close: Some("0.3".parse().unwrap()),
            volume_traded: "12.5".parse().unwrap(),
            trades_count: 2,
            time_period_start: "2026-10-11T00:03:00Z".parse().unwrap(),
            time_period_end: "2026-10-11T00:04:00Z".parse().unwrap(),
            time_open: Some("2026-10-11T00:03:10.0000001Z".parse().unwrap()),
            time_close: Some("2026-10-11T00:03:50.0000003Z".parse().unwrap()),
        }]
    );

    let latest_query = OhlcvQuery {
        time_start: None,
        time_end: None,
        ..ohlcv()
    };

    assert_eq!(
        client
            .get_ohlcv_latest("KALSHI", id, &latest_query)
            .await
            .unwrap(),
        vec![]
    );
    let trades = client
        .get_trades_history("KALSHI", id, &history())
        .await
        .unwrap();
    assert_eq!(trades.len(), 3);
    let quotes = client
        .get_quotes_history("POLYMARKET", id, &history())
        .await
        .unwrap();
    assert_eq!(
        quotes[0].recv_time.as_nanosecond() - quotes[0].entry_time.as_nanosecond(),
        200
    );

    let dated = HistoryQuery {
        date: Some("2026-10-11".parse().unwrap()),
        time_start: None,
        time_end: None,
        limit: 3,
    };

    assert_eq!(
        client
            .get_trades_history("KALSHI", id, &dated)
            .await
            .unwrap(),
        trades
    );
    assert_eq!(
        client.get_current("POLYMARKET", id).await.unwrap(),
        ActivityCurrent {
            quote: None,
            trade: None,
        }
    );
    let requests = peer.await.unwrap();
    let m = "opaque%2FNO%3F+=%25";
    let times = "time_start=2026-10-11T00%3A00%3A00.0000001Z&time_end=2026-10-12T00%3A00%3A00.0000003Z&limit=3";
    let targets = vec![
        "/v1/exchanges/POLYMARKET".into(),
        format!("/v1/markets/POLYMARKET/{m}"),
        format!("/v1/activity/POLYMARKET/{m}/current"),
        format!("/v1/activity/POLYMARKET/{m}/latest?limit=3"),
        format!("/v1/orderbook/POLYMARKET/{m}/current"),
        "/v1/ohlcv/periods".into(),
        format!(
            "/v1/ohlcv/KALSHI/{m}/history?period_id=1MIN&{}",
            times.replace("limit=3", "limit=1")
        ),
        format!("/v1/ohlcv/KALSHI/{m}/latest?period_id=1MIN&limit=1"),
        format!("/v1/activity/KALSHI/{m}/history/trades?{times}"),
        format!("/v1/activity/POLYMARKET/{m}/history/quotes?{times}"),
        format!("/v1/activity/KALSHI/{m}/history/trades?date=2026-10-11&limit=3"),
        format!("/v1/activity/POLYMARKET/{m}/current"),
    ];
    assert_eq!(requests.len(), targets.len());

    for (request, target) in requests.iter().zip(targets) {
        assert_request(request, &target, &config);
    }

    assert_eq!(client.attempts_remaining(), 88);
    #[cfg(all(feature = "simulation", madsim))]
    println!(
        "wire: {requests:?}\nmodel: {} {:?} {:?} {:?}",
        market.market_id.as_deref().unwrap(),
        bars,
        trades,
        quotes
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn published_examples_preserve_discovery_and_raw_response_fields() {
    let market_payload = include_str!("../../test_data/published/market.json");
    let listings_payload = include_str!("../../test_data/published/markets.json");
    let listings_envelope = format!("{{\"data\":{listings_payload},\"next_cursor\":null}}");
    let (peer, config) = start_peer(
        vec![
            EXCHANGES.to_owned(),
            include_str!("../../test_data/published/exchange.json").to_owned(),
            listings_envelope,
            include_str!("../../test_data/published/active.json").to_owned(),
            market_payload.to_owned(),
            include_str!("../../test_data/published/periods.json").to_owned(),
            include_str!("../../test_data/published/book.json").to_owned(),
        ]
        .into_iter()
        .map(|body| (200, "", body))
        .collect(),
    )
    .await;
    let client = new_client(config.clone());
    let exchanges = client.get_exchanges().await.unwrap();
    let exchange = client.get_exchange("POLYMARKET").await.unwrap();

    let page = client
        .get_listings(
            "POLYMARKET",
            &ListingsQuery {
                limit: 2,
                cursor: None,
            },
        )
        .await
        .unwrap();

    let active = client
        .get_active("POLYMARKET", &ActiveQuery { limit: 5, page: 1 })
        .await
        .unwrap();
    let id = "WILL-TRUMP-SELL-10K-25K-GOLD-CARDS-IN-2025_YES";
    let market = client.get_market("POLYMARKET", id).await.unwrap();
    let periods = client.get_periods().await.unwrap();
    let book = client.get_book("POLYMARKET", "market_yes").await.unwrap();
    let requests = peer.await.unwrap();

    let raw: Vec<serde_json::Value> = serde_json::from_str(EXCHANGES).unwrap();

    let expected: Vec<_> = raw
        .iter()
        .map(|value| Exchange {
            exchange_id: value["exchange_id"].as_str().map(Into::into),
            market_name_institution_description: value["market_name_institution_description"]
                .as_str()
                .map(str::to_owned),
            website: value["website"].as_str().map(str::to_owned),
        })
        .collect();

    assert_eq!(exchanges, expected);
    assert_eq!(
        exchange,
        Exchange {
            exchange_id: Some("POLYMARKET".into()),
            market_name_institution_description: Some(
                "The world's largest prediction market.".into()
            ),
            website: Some("https://polymarket.com".into()),
        }
    );
    assert_eq!(page.next_cursor, None);
    let listings = page.data.unwrap();
    let raw_listings: Vec<serde_json::Value> = serde_json::from_str(listings_payload).unwrap();
    let raw_market: serde_json::Value = serde_json::from_str(market_payload).unwrap();
    assert_eq!(listings.len(), 2);

    for (actual, expected) in listings
        .iter()
        .chain(std::iter::once(&market))
        .zip(raw_listings.iter().chain(std::iter::once(&raw_market)))
    {
        assert_eq!(actual.market_id.as_deref(), expected["market_id"].as_str());
        assert_eq!(actual.title.as_deref(), expected["title"].as_str());
        assert_eq!(
            actual.description.as_deref(),
            expected["description"].as_str()
        );
        assert_eq!(
            actual.outcome_name.as_deref(),
            expected["outcome_name"].as_str()
        );
        assert_eq!(actual.price, expected["price"].to_string().parse().unwrap());
        assert_eq!(actual.status, MarketStatus::Open);
        assert_eq!(
            actual.exchange_id.as_deref(),
            expected["exchange_id"].as_str()
        );
        assert_eq!(actual.outcome_type, OutcomeType::Binary);
        assert_eq!(
            actual.mechanism.as_ref().map(AsRef::as_ref),
            expected["mechanism"].as_str()
        );
        assert_eq!(
            actual.exchange.as_ref(),
            expected.get("exchange").map(|_| &exchange)
        );

        let source = actual.source_specific_data.as_ref().map(|values| {
            values
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        serde_json::from_str::<serde_json::Value>(value.get()).unwrap(),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
        });

        assert_eq!(
            source
                .as_ref()
                .map(|value| serde_json::Value::Object(value.clone())),
            expected.get("source_specific_data").cloned()
        );
    }

    assert!(matches!(
        parse_instrument(&market, UnixNanos::default()),
        Err(Error::Definition {
            field: "ConditionId",
            reason: "is missing",
            ..
        })
    ));
    assert_eq!(
        active,
        vec![
            id.to_owned(),
            "WILL-TRUMP-SELL-10K-25K-GOLD-CARDS-IN-2025_NO".into(),
            "NFL-PIT-CAR-2025-08-21-SPREAD-AWAY-5PT5_YES".into(),
            "NFL-PIT-CAR-2025-08-21-SPREAD-AWAY-5PT5_NO".into(),
            "CRYPTO-BTC-PRICE-ABOVE-100K-BY-EOY-2025_YES".into(),
        ]
    );
    assert_eq!(
        periods,
        vec![
            OhlcvPeriod {
                period_id: Some("1SEC".into()),
                unit_name: Some("second".into()),
                unit_count: Some(1),
                length_seconds: 1,
                length_months: 0,
                display_name: Some("1 Second".into()),
            },
            OhlcvPeriod {
                period_id: Some("10DAY".into()),
                unit_name: Some("day".into()),
                unit_count: Some(10),
                length_seconds: 864000,
                length_months: 0,
                display_name: Some("10 Days".into()),
            },
            OhlcvPeriod {
                period_id: Some("2YRS".into()),
                unit_name: Some("year".into()),
                unit_count: Some(2),
                length_seconds: 0,
                length_months: 24,
                display_name: Some("2 Years".into()),
            },
        ]
    );
    assert_eq!(book.exchange_id.as_deref(), Some("POLYMARKET"));
    assert_eq!(book.market_id.as_deref(), Some("market_yes"));
    assert_eq!(book.time_exchange, "0001-01-01T00:00:00Z".parse().unwrap());
    assert_eq!(book.time_coinapi, "0001-01-01T00:00:00Z".parse().unwrap());
    assert_eq!(book.asks, Some(vec![]));
    assert_eq!(book.bids, Some(vec![]));
    assert_eq!(requests.len(), 7);

    for (request, target) in requests.iter().zip([
        "/v1/exchanges".to_owned(),
        "/v1/exchanges/POLYMARKET".into(),
        "/v1/markets/POLYMARKET/history?limit=2".into(),
        "/v1/markets/POLYMARKET/active?limit=5&page=1".into(),
        format!("/v1/markets/POLYMARKET/{id}"),
        "/v1/ohlcv/periods".into(),
        "/v1/orderbook/POLYMARKET/market_yes/current".into(),
    ]) {
        assert_request(request, &target, &config);
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cursor_and_active_pagination_terminate_only_at_terminal_page() {
    let null_data = TERMINAL.replace("[]", "null");

    for terminal in [
        TERMINAL,
        include_str!("../../test_data/missing-cursor.json"),
        &null_data,
    ] {
        let (peer, config) = start_peer(vec![
            (200, "", LISTINGS.to_owned()),
            (200, "", LISTINGS_NEXT.to_owned()),
            (200, "", terminal.to_owned()),
            (200, "", ACTIVE_ONE.to_owned()),
            (200, "", EMPTY.to_owned()),
        ])
        .await;
        let client = new_client(config.clone());

        let bounds = Pagination {
            max_pages: 3,
            max_records: 4,
        };

        let markets = client
            .get_listings_all(
                "POLYMARKET",
                &ListingsQuery {
                    limit: 2,
                    cursor: None,
                },
                bounds,
            )
            .await
            .unwrap();

        assert_eq!(markets.len(), 4);
        assert_ne!(markets[0].market_id, markets[2].market_id);
        let ids = client
            .get_active_all("POLYMARKET", &ActiveQuery { limit: 2, page: 1 }, bounds)
            .await
            .unwrap();
        let requests = peer.await.unwrap();
        assert_eq!(ids, vec!["opaque/ID?+=%"]);
        assert_eq!(requests.len(), 5);
        assert_request(
            &requests[0],
            "/v1/markets/POLYMARKET/history?limit=2",
            &config,
        );
        let first: serde_json::Value = serde_json::from_str(LISTINGS).unwrap();
        let second: serde_json::Value = serde_json::from_str(LISTINGS_NEXT).unwrap();
        for (request, page) in [(&requests[1], first), (&requests[2], second)] {
            let cursor = page["next_cursor"].as_str().unwrap();
            let encoded: String = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("cursor", cursor)
                .finish();
            assert_request(
                request,
                &format!("/v1/markets/POLYMARKET/history?limit=2&{encoded}"),
                &config,
            );
        }

        assert_request(
            &requests[3],
            "/v1/markets/POLYMARKET/active?limit=2&page=1",
            &config,
        );
        assert_request(
            &requests[4],
            "/v1/markets/POLYMARKET/active?limit=2&page=2",
            &config,
        );
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn pagination_rejects_cycles_duplicates_and_bounds() {
    for (responses, bounds, expected) in [
        (
            vec![(200, "", CYCLE), (200, "", CYCLE)],
            Pagination {
                max_pages: 3,
                max_records: 4,
            },
            "listing cursor is empty or repeated",
        ),
        (
            vec![(200, "", CYCLE)],
            Pagination {
                max_pages: 1,
                max_records: 4,
            },
            "listing page bound exceeded",
        ),
        (
            vec![(200, "", LISTINGS)],
            Pagination {
                max_pages: 3,
                max_records: 1,
            },
            "listing record bound exceeded",
        ),
    ] {
        let (peer, config) = start_peer(responses).await;

        let e = new_client(config)
            .get_listings_all(
                "POLYMARKET",
                &ListingsQuery {
                    limit: 2,
                    cursor: None,
                },
                bounds,
            )
            .await
            .unwrap_err();

        assert!(matches!(e,Error::Response(message) if message==expected));
        peer.await.unwrap();
    }

    let (peer, config) = start_peer(vec![(200, "", ACTIVE_ONE), (200, "", ACTIVE_ONE)]).await;

    let e = new_client(config)
        .get_active_all(
            "POLYMARKET",
            &ActiveQuery { limit: 2, page: 1 },
            Pagination {
                max_pages: 3,
                max_records: 4,
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(
        e,
        Error::Response("active ID is empty or repeated")
    ));
    assert_eq!(peer.await.unwrap().len(), 2);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn retries_preserve_headers_and_honor_retry_after_and_local_spacing() {
    let (peer, mut config) = start_peer(vec![
        (429, "Retry-After: 1\r\n", EMPTY),
        (503, "", EMPTY),
        (200, "", EXCHANGES),
    ])
    .await;
    config.min_request_interval_ms = 50;
    let client = new_client(config.clone());
    let start = time::Instant::now();
    assert_eq!(client.get_exchanges().await.unwrap().len(), 9);
    let elapsed = start.elapsed();
    let requests = peer.await.unwrap();
    assert!(
        elapsed >= Duration::from_millis(1050),
        "elapsed {elapsed:?}"
    );
    assert_eq!(client.attempts_remaining(), 97);
    assert_eq!(requests.len(), 3);

    for request in requests {
        assert_request(&request, "/v1/exchanges", &config);
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn permanent_status_redirect_and_decode_errors_never_retry() {
    for (status, body) in [
        (401, EMPTY),
        (403, EMPTY),
        (400, EMPTY),
        (404, EMPTY),
        (302, EMPTY),
        (501, EMPTY),
        (200, include_str!("../../test_data/malformed.txt")),
        (200, include_str!("../../test_data/echo-secret.json")),
    ] {
        let (peer, config) = start_peer(vec![(
            status,
            "Location: http://127.0.0.1:9/secret\r\n",
            body,
        )])
        .await;
        let client = new_client(config);
        let e = client.get_exchanges().await.unwrap_err();
        assert!(!e.is_retryable());
        assert!(!format!("{e:?} {e}").contains("test-api-key"));
        assert_eq!(client.attempts_remaining(), 99);
        assert_eq!(peer.await.unwrap().len(), 1);
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn budget_cancellation_and_elapsed_deadline_bound_admission() {
    let (peer, mut config) = start_peer(vec![(200, "", EXCHANGES)]).await;
    config.attempt_budget = 1;
    let client = new_client(config);
    client.get_exchanges().await.unwrap();
    assert!(matches!(
        client.get_exchanges().await,
        Err(Error::AttemptBudget)
    ));
    client.cancel();
    assert!(matches!(
        client.get_exchanges().await,
        Err(Error::Retry(RetryError::Canceled))
    ));
    assert_eq!(client.attempts_remaining(), 0);
    assert_eq!(peer.await.unwrap().len(), 1);

    let (peer, mut config) = start_peer(vec![(200, "", EXCHANGES)]).await;
    config.min_request_interval_ms = 200;
    config.retry.max_elapsed_ms = Some(40);
    config.retry.operation_timeout_ms = Some(1_000);
    let client = Arc::new(new_client(config));
    client.get_exchanges().await.unwrap();
    let start = time::Instant::now();
    assert!(matches!(
        client.get_exchanges().await,
        Err(Error::Retry(RetryError::ElapsedBudgetExceeded { .. }))
    ));
    assert!(start.elapsed() >= Duration::from_millis(40));
    assert_eq!(client.attempts_remaining(), 99);
    assert_eq!(peer.await.unwrap().len(), 1);

    let pending = task::spawn({
        let client = Arc::clone(&client);
        async move { client.get_exchanges().await }
    });

    time::sleep(Duration::from_millis(5)).await;
    client.cancel();
    assert!(matches!(
        pending.await.unwrap(),
        Err(Error::Retry(RetryError::Canceled))
    ));
}

#[rstest]
#[case("https://public.example:8443/path-sentinel/v1/")]
#[case(
    "https://user-sentinel:password-sentinel@public.example:8443/path-sentinel?key=query-sentinel#fragment-sentinel"
)]
#[case(
    "https://user-sentinel:password-sentinel@[invalid/path-sentinel?query-sentinel#fragment-sentinel"
)]
#[case("file:///path-sentinel?query-sentinel#fragment-sentinel")]
fn endpoint_config_debug_redacts_before_validation(#[case] base: &str) {
    let http = FinFeedHttpConfig {
        base_url: base.into(),
        proxy_url: Some("http://proxy-user:proxy-sentinel@localhost:8124".into()),
        ..Default::default()
    };

    let config = FinFeedDataClientConfig {
        api_key: Some("api-sentinel".into()),
        http,
        ..Default::default()
    };

    for rendered in [
        format!("{:?}", config.http),
        format!("{:#?}", config.http),
        format!("{config:?}"),
        format!("{config:#?}"),
    ] {
        assert!(rendered.contains("base_url: <redacted>"), "{rendered}");

        for secret in [
            "path-sentinel",
            "user-sentinel",
            "password-sentinel",
            "query-sentinel",
            "fragment-sentinel",
            "public.example",
            "api-sentinel",
            "proxy-sentinel",
            "proxy-user",
        ] {
            assert!(!rendered.contains(secret), "{rendered}");
        }
    }

    assert_eq!(config.http.base_url.expose_secret(), base);
    let serialized = serde_json::to_value(&config).unwrap();
    assert_eq!(serialized["http"]["base_url"], base);
    let decoded: FinFeedDataClientConfig = serde_json::from_value(serialized).unwrap();
    assert_eq!(decoded.http.base_url.expose_secret(), base);
}

#[rstest]
fn endpoint_client_debug_redacts_nested_surfaces() {
    let config = FinFeedDataClientConfig {
        api_key: Some("api-sentinel".into()),
        http: FinFeedHttpConfig {
            base_url: "https://public.example:8443/path-sentinel/v1/".into(),
            ..Default::default()
        },
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec!["TEST-OUTCOME_NO".into()],
        }],
        ..Default::default()
    };

    let http =
        Arc::new(FinFeedHttpClient::new(config.api_key.clone(), config.http.clone()).unwrap());
    let provider = FinFeedInstrumentProvider::new(Arc::clone(&http));
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let client = FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();

    for rendered in [
        format!("{http:?}"),
        format!("{provider:?}"),
        format!("{client:?}"),
        format!("{client:#?}"),
    ] {
        assert!(
            rendered.contains("base_url: \"https://public.example:8443\""),
            "{rendered}"
        );

        for secret in ["path-sentinel", "api-sentinel"] {
            assert!(!rendered.contains(secret), "{rendered}");
        }
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn endpoint_debug_preserves_request_path() {
    let (peer, mut config) = start_peer(vec![(200, "", EXCHANGES)]).await;
    config.base_url = config
        .base_url
        .expose_secret()
        .replace("/v1/", "/path-sentinel%2Fexact/v1")
        .into();
    let client = new_client(config);
    let rendered = format!("{client:?}");
    let exchanges = client.get_exchanges().await.unwrap();
    let requests = time::timeout(Duration::from_secs(3), peer)
        .await
        .unwrap()
        .unwrap();

    assert!(!rendered.contains("path-sentinel"));
    assert_eq!(exchanges.len(), 9);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].lines().next().unwrap(),
        "GET /path-sentinel%2Fexact/v1/exchanges HTTP/1.1"
    );
}

#[rstest]
fn http_builder_and_serde_defaults_match() {
    let expected = serde_json::to_value(FinFeedHttpConfig::default()).unwrap();
    let built = FinFeedHttpConfig::builder().build();
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let decoded: FinFeedHttpConfig = serde_json::from_value(empty).unwrap();

    assert_eq!(serde_json::to_value(built).unwrap(), expected);
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[rstest]
fn construction_rejects_invalid_credentials_urls_and_controls() {
    for key in ["", "has space", "tab\tkey", "non-ascii-é"] {
        let e = FinFeedHttpClient::new(Some(SecretString::from(key)), FinFeedHttpConfig::default())
            .unwrap_err();
        assert!(matches!(e, Error::Validation(_)));
    }

    for base in [
        "http://example.com/v1/",
        "ftp://127.0.0.1",
        "https://user:test-api-key@example.com/",
        "https://example.com/?key=test-api-key",
        "https://example.com/#fragment",
    ] {
        let e = FinFeedHttpClient::new(
            Some(SecretString::from("test-api-key")),
            FinFeedHttpConfig {
                base_url: base.into(),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(!format!("{e:?} {e}").contains("test-api-key"));
    }

    for proxy in [
        "socks5://127.0.0.1:9",
        "http://user:proxy-secret@[::1",
        "http://127.0.0.1:9/path",
        "http://127.0.0.1:9/?key=proxy-secret",
    ] {
        let config = FinFeedHttpConfig {
            proxy_url: Some(SecretString::from(proxy)),
            ..Default::default()
        };

        let e =
            FinFeedHttpClient::new(Some(SecretString::from("test-api-key")), config).unwrap_err();
        assert!(matches!(e, Error::Validation(_)));
        assert!(!format!("{e:?} {e}").contains("proxy-secret"));
    }

    for change in 0..14 {
        let mut config = FinFeedHttpConfig::default();
        match change {
            0 => config.timeout_secs = 0,
            1 => config.max_concurrent_requests = 0,
            2 => config.attempt_budget = 0,
            3 => config.min_request_interval_ms = 0,
            4 => config.retry.operation_timeout_ms = Some(0),
            5 => config.retry.max_elapsed_ms = None,
            6 => config.retry.initial_delay_ms = 0,
            7 => config.retry.max_delay_ms = 0,
            8 => config.retry.backoff_factor = f64::NAN,
            9 => config.retry.max_retries = u32::MAX,
            10 => config.timeout_secs = 86_401,
            11 => config.max_concurrent_requests = tokio::sync::Semaphore::MAX_PERMITS + 1,
            12 => config.retry.operation_timeout_ms = None,
            13 => config.retry.max_delay_ms = 86_400_001,
            _ => unreachable!(),
        }

        assert!(
            FinFeedHttpClient::new(Some(SecretString::from("key")), config).is_err(),
            "case {change}"
        );
    }
}

#[rstest]
#[case(1_000)]
#[case(1_001)]
#[case(10_000)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn activity_limits_apply_independently_to_each_list(#[case] limit: u32) {
    let template: serde_json::Value =
        serde_json::from_str(include_str!("../../test_data/poll-activity.json")).unwrap();
    let mut body = template.clone();
    body["quotes"] = serde_json::Value::Array(vec![template["quotes"][0].clone(); limit as usize]);
    body["trades"] = serde_json::Value::Array(vec![template["trades"][0].clone(); limit as usize]);
    let expected: ActivityBatch = serde_json::from_value(body.clone()).unwrap();
    let mut too_many_quotes = body.clone();
    too_many_quotes["quotes"]
        .as_array_mut()
        .unwrap()
        .push(template["quotes"][0].clone());
    let mut too_many_trades = body.clone();
    too_many_trades["trades"]
        .as_array_mut()
        .unwrap()
        .push(template["trades"][0].clone());
    let (peer, config) = start_peer(vec![
        (200, "", body.to_string()),
        (200, "", too_many_quotes.to_string()),
        (200, "", too_many_trades.to_string()),
    ])
    .await;
    let client = new_client(config.clone());
    let query = ActivityQuery { limit };
    let actual = client
        .get_latest("POLYMARKET", "capacity_NO", &query)
        .await
        .unwrap();

    for _ in 0..2 {
        assert!(matches!(
            client.get_latest("POLYMARKET", "capacity_NO", &query).await,
            Err(Error::Response(_))
        ));
    }

    let requests = peer.await.unwrap();
    assert_eq!(actual, expected);
    assert_eq!(client.attempts_remaining(), config.attempt_budget - 3);
    assert_eq!(requests.len(), 3);

    for request in requests {
        assert_request(
            &request,
            &format!("/v1/activity/POLYMARKET/capacity_NO/latest?limit={limit}"),
            &config,
        );
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_queries_fail_before_transmission() {
    let client = new_client(FinFeedHttpConfig {
        base_url: "http://127.0.0.1:9/v1/".into(),
        ..Default::default()
    });

    for id in ["", ".", "..", "   "] {
        assert!(matches!(
            client.get_market("POLYMARKET", id).await,
            Err(Error::Validation(_))
        ));
    }

    for limit in [0, PAGE_LIMIT_MAX + 1] {
        assert!(matches!(
            client
                .get_latest("POLYMARKET", "id", &ActivityQuery { limit })
                .await,
            Err(Error::Validation(_))
        ));
        assert!(matches!(
            client
                .get_listings(
                    "POLYMARKET",
                    &ListingsQuery {
                        limit,
                        cursor: None
                    }
                )
                .await,
            Err(Error::Validation(_))
        ));
        assert!(matches!(
            client
                .get_active("POLYMARKET", &ActiveQuery { limit, page: 1 })
                .await,
            Err(Error::Validation(_))
        ));
    }

    for page in [0, i32::MAX as u32 + 1] {
        assert!(matches!(
            client
                .get_active("POLYMARKET", &ActiveQuery { limit: 1, page })
                .await,
            Err(Error::Validation(_))
        ));
    }

    for limit in [0, PAGE_LIMIT_MAX + 1] {
        let query = HistoryQuery { limit, ..history() };
        assert!(matches!(
            client.get_trades_history("KALSHI", "id", &query).await,
            Err(Error::Validation(_))
        ));
    }

    let mut query = history();
    query.date = Some("2026-10-11".parse().unwrap());
    assert!(matches!(
        client.get_quotes_history("POLYMARKET", "id", &query).await,
        Err(Error::Validation(_))
    ));
    query = history();
    query.time_end = query.time_start;
    assert!(matches!(
        client.get_quotes_history("POLYMARKET", "id", &query).await,
        Err(Error::Validation(_))
    ));
    query = history();
    query.time_start = Some("2026-10-11T00:00:00.000000001Z".parse().unwrap());
    assert!(matches!(
        client.get_quotes_history("POLYMARKET", "id", &query).await,
        Err(Error::Validation(_))
    ));
    query = history();
    query.time_start = None;
    assert!(matches!(
        client.get_quotes_history("POLYMARKET", "id", &query).await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        client.get_ohlcv_latest("KALSHI", "id", &ohlcv()).await,
        Err(Error::Validation(_))
    ));

    for limit in [0, OHLCV_LIMIT_MAX + 1] {
        let query = OhlcvQuery { limit, ..ohlcv() };
        assert!(matches!(
            client.get_ohlcv_history("KALSHI", "id", &query).await,
            Err(Error::Validation(_))
        ));
    }

    let query = OhlcvQuery {
        period_id: "".into(),
        ..ohlcv()
    };

    assert!(matches!(
        client.get_ohlcv_history("KALSHI", "id", &query).await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        client
            .get_listings(
                "POLYMARKET",
                &ListingsQuery {
                    limit: 1,
                    cursor: Some(String::new())
                }
            )
            .await,
        Err(Error::Validation(message)) if message == "cursor must not be empty"
    ));

    for bounds in [
        Pagination {
            max_pages: 0,
            max_records: 1,
        },
        Pagination {
            max_pages: 1,
            max_records: 0,
        },
    ] {
        assert!(matches!(
            client
                .get_listings_all(
                    "POLYMARKET",
                    &ListingsQuery {
                        limit: 1,
                        cursor: None
                    },
                    bounds
                )
                .await,
            Err(Error::Validation(_))
        ));
    }

    assert_eq!(client.attempts_remaining(), 100);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn malformed_overflow_and_overfull_pages_fail_at_transport_boundary() {
    for body in [
        include_str!("../../test_data/overflow-current.json"),
        include_str!("../../test_data/underflow-current.json"),
    ] {
        let (peer, config) = start_peer(vec![(200, "", body)]).await;
        let client = new_client(config);
        assert!(matches!(
            client.get_current("POLYMARKET", "id").await,
            Err(Error::Parse { .. })
        ));
        assert_eq!(client.attempts_remaining(), 99);
        peer.await.unwrap();
    }

    let (peer, config) = start_peer(vec![(200, "", LISTINGS)]).await;
    assert!(matches!(
        new_client(config)
            .get_listings(
                "POLYMARKET",
                &ListingsQuery {
                    limit: 1,
                    cursor: None
                }
            )
            .await,
        Err(Error::Response("response exceeds requested record limit"))
    ));
    peer.await.unwrap();
}

#[rstest]
#[case("active")]
#[case("trades")]
#[case("quotes")]
#[case("ohlcv_history")]
#[case("ohlcv_latest")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn overfull_typed_responses_fail_after_admission(#[case] endpoint: &str) {
    let body = match endpoint {
        "active" => r#"["first_NO","second_NO"]"#.to_owned(),
        "trades" => TRADES.to_owned(),
        "quotes" => QUOTES.to_owned(),
        "ohlcv_history" | "ohlcv_latest" => {
            let mut rows: Vec<serde_json::Value> = serde_json::from_str(BARS).unwrap();
            rows.push(rows[0].clone());
            serde_json::to_string(&rows).unwrap()
        }
        _ => unreachable!(),
    };
    let (peer, config) = start_peer(vec![(200, "", body)]).await;
    let client = new_client(config);
    let history = HistoryQuery {
        limit: 2,
        ..history()
    };

    let result = match endpoint {
        "active" => client
            .get_active("POLYMARKET", &ActiveQuery { limit: 1, page: 1 })
            .await
            .map(|_| ()),
        "trades" => client
            .get_trades_history("KALSHI", "id", &history)
            .await
            .map(|_| ()),
        "quotes" => client
            .get_quotes_history("POLYMARKET", "id", &history)
            .await
            .map(|_| ()),
        "ohlcv_history" => client
            .get_ohlcv_history("KALSHI", "id", &ohlcv())
            .await
            .map(|_| ()),
        "ohlcv_latest" => client
            .get_ohlcv_latest(
                "KALSHI",
                "id",
                &OhlcvQuery {
                    time_start: None,
                    time_end: None,
                    ..ohlcv()
                },
            )
            .await
            .map(|_| ()),
        _ => unreachable!(),
    };

    assert!(matches!(
        result,
        Err(Error::Response("response exceeds requested record limit"))
    ));
    assert_eq!(client.attempts_remaining(), 99);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg(not(all(feature = "simulation", madsim)))]
#[tokio::test]
async fn explicit_http_proxy_routes_and_authenticates() {
    let (peer, proxy) = start_peer(vec![(200, "", EXCHANGES)]).await;

    let config = FinFeedHttpConfig {
        base_url: "http://127.0.0.1:9/v1/".into(),
        proxy_url: Some(SecretString::from(
            proxy
                .base_url
                .expose_secret()
                .replace("/v1/", "/")
                .replace("http://", "http://user:pass@"),
        )),
        ..proxy
    };

    let client = new_client(config);
    assert!(!format!("{client:?}").contains("user:pass"));
    assert_eq!(client.get_exchanges().await.unwrap().len(), 9);
    let request = &peer.await.unwrap()[0];
    assert_eq!(
        request.lines().next().unwrap(),
        "GET http://127.0.0.1:9/v1/exchanges HTTP/1.1"
    );
    assert!(request.contains("\r\nproxy-authorization: Basic dXNlcjpwYXNz\r\n"));
    assert!(request.contains("\r\nauthorization: test-api-key\r\n"));
    assert!(request.contains("\r\nx-referrer: nautilus\r\n"));
}

#[cfg(not(all(feature = "simulation", madsim)))]
#[tokio::test]
async fn ambient_proxies_are_disabled() {
    const CHILD: &str = "FINFEED_PROXY_TEST_CHILD";

    if std::env::var_os(CHILD).is_none() {
        let exe = std::env::current_exe().unwrap();
        let status = std::process::Command::new(exe)
            .args([
                "--exact",
                "dst::ambient_proxies_are_disabled",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("http_proxy", "http://127.0.0.1:9")
            .env("https_proxy", "http://127.0.0.1:9")
            .env("all_proxy", "http://127.0.0.1:9")
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }

    let (peer, config) = start_peer(vec![(200, "", EXCHANGES)]).await;
    assert_eq!(new_client(config).get_exchanges().await.unwrap().len(), 9);
    assert_eq!(peer.await.unwrap().len(), 1);
}

pub(super) async fn bind_peer() -> (TcpListener, String) {
    #[cfg(not(all(feature = "simulation", madsim)))]
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    #[cfg(all(feature = "simulation", madsim))]
    let listener = TcpListener::bind("127.0.0.1:18896").await.unwrap();
    #[cfg(not(all(feature = "simulation", madsim)))]
    let address = listener.local_addr().unwrap().to_string();
    #[cfg(all(feature = "simulation", madsim))]
    let address = "127.0.0.1:18896".to_owned();
    (listener, address)
}

pub(super) async fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await.unwrap());
    }

    String::from_utf8(bytes).unwrap()
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn configured_timeouts_and_retry_count_bound_inflight_requests() {
    for (transport, operation) in [(1, 1500), (30, 20)] {
        let (peer, mut config) =
            start_peer_delayed(vec![(200, "", EXCHANGES)], Duration::from_secs(2)).await;
        config.timeout_secs = transport;
        config.retry.max_retries = 0;
        config.retry.operation_timeout_ms = Some(operation);
        config.retry.max_elapsed_ms = Some(3_000);
        let client = new_client(config);
        let start = time::Instant::now();
        let e = client.get_exchanges().await.unwrap_err();
        let elapsed = start.elapsed();

        if transport == 1 {
            assert!(matches!(e, Error::Timeout));
            assert!(elapsed >= Duration::from_secs(1));
        } else {
            assert!(matches!(
                e,
                Error::Retry(RetryError::OperationTimeout { timeout_ms: 20 })
            ));
            assert!(elapsed >= Duration::from_millis(20));
        }

        assert!(elapsed < Duration::from_millis(1600));
        assert_eq!(client.attempts_remaining(), 99);
        peer.abort();
    }

    let (peer, mut config) =
        start_peer(vec![(503, "", EMPTY), (503, "", EMPTY), (503, "", EMPTY)]).await;
    config.retry.max_retries = 2;
    let client = new_client(config);
    assert!(matches!(
        client.get_exchanges().await,
        Err(Error::Http { status: 503, .. })
    ));
    assert_eq!(client.attempts_remaining(), 97);
    assert_eq!(peer.await.unwrap().len(), 3);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_interrupts_inflight_and_retry_waits() {
    for retry_wait in [false, true] {
        let (peer, mut config) = if retry_wait {
            start_peer(vec![(503, "Retry-After: 3\r\n", EMPTY)]).await
        } else {
            start_peer_delayed(vec![(200, "", EXCHANGES)], Duration::from_secs(1)).await
        };

        config.retry.max_elapsed_ms = Some(5_000);
        let client = Arc::new(new_client(config));

        let pending = task::spawn({
            let client = Arc::clone(&client);
            async move { client.get_exchanges().await }
        });

        time::sleep(Duration::from_millis(30)).await;
        client.cancel();
        assert!(matches!(
            pending.await.unwrap(),
            Err(Error::Retry(RetryError::Canceled))
        ));
        assert_eq!(client.attempts_remaining(), 99);
        peer.abort();
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn configured_concurrency_limits_actual_inflight_transport() {
    for concurrency in [1, 2] {
        let (listener, address) = bind_peer().await;

        let config = FinFeedHttpConfig {
            base_url: format!("http://{address}/v1/").into(),
            min_request_interval_ms: 1,
            max_concurrent_requests: concurrency,
            ..Default::default()
        };

        let server = task::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            read_request(&mut first).await;
            let early = time::timeout(Duration::from_millis(50), listener.accept()).await;
            let overlapped = early.is_ok();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{EXCHANGES}",
                EXCHANGES.len()
            );
            first.write_all(response.as_bytes()).await.unwrap();
            first.flush().await.unwrap();

            let (mut second, _) = match early {
                Ok(result) => result.unwrap(),
                Err(_) => listener.accept().await.unwrap(),
            };

            read_request(&mut second).await;
            second.write_all(response.as_bytes()).await.unwrap();
            second.flush().await.unwrap();
            overlapped
        });

        let client = Arc::new(new_client(config));

        let first = task::spawn({
            let client = Arc::clone(&client);
            async move { client.get_exchanges().await }
        });

        let second = task::spawn({
            let client = Arc::clone(&client);
            async move { client.get_exchanges().await }
        });

        assert_eq!(first.await.unwrap().unwrap().len(), 9);
        assert_eq!(second.await.unwrap().unwrap().len(), 9);
        assert_eq!(server.await.unwrap(), concurrency == 2);
        assert_eq!(client.attempts_remaining(), 98);
    }
}

#[cfg(not(all(feature = "simulation", madsim)))]
#[tokio::test]
async fn explicit_https_proxy_uses_tls_and_http_proxy_tunnels_https() {
    let (listener, address) = bind_peer().await;
    let tls = task::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut header = [0u8; 5];
        stream.read_exact(&mut header).await.unwrap();
        header
    });

    let mut config = FinFeedHttpConfig {
        proxy_url: Some(SecretString::from(format!("https://{address}"))),
        ..Default::default()
    };

    config.retry.max_retries = 0;
    assert!(new_client(config).get_exchanges().await.is_err());
    let tls_header = tls.await.unwrap();
    assert_eq!(tls_header[0], 22);
    assert_eq!(tls_header[1], 3);

    let (peer, proxy) = start_peer(vec![(407, "", EMPTY)]).await;

    let mut config = FinFeedHttpConfig {
        proxy_url: Some(SecretString::from(
            proxy.base_url.expose_secret().replace("/v1/", "/"),
        )),
        ..Default::default()
    };

    config.retry.max_retries = 0;
    assert!(new_client(config).get_exchanges().await.is_err());
    let request = &peer.await.unwrap()[0];
    assert_eq!(
        request.lines().next().unwrap(),
        "CONNECT api.prediction-markets.finfeedapi.com:443 HTTP/1.1"
    );
    assert!(!request.contains("test-api-key"));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn pagination_enforces_active_bounds_and_opaque_start_cursor() {
    for (bounds, page, expected) in [
        (
            Pagination {
                max_pages: 1,
                max_records: 4,
            },
            1,
            "active page bound exceeded",
        ),
        (
            Pagination {
                max_pages: 2,
                max_records: 1,
            },
            1,
            "active ID record bound exceeded",
        ),
        (
            Pagination {
                max_pages: 2,
                max_records: 4,
            },
            i32::MAX as u32,
            "active page overflow",
        ),
    ] {
        let responses = if expected == "active ID record bound exceeded" {
            vec![(200, "", ACTIVE_ONE), (200, "", ACTIVE_ONE)]
        } else {
            vec![(200, "", ACTIVE_ONE)]
        };

        let (peer, config) = start_peer(responses).await;
        let e = new_client(config)
            .get_active_all("POLYMARKET", &ActiveQuery { limit: 2, page }, bounds)
            .await
            .unwrap_err();
        assert!(matches!(e,Error::Response(message) if message==expected));
        peer.await.unwrap();
    }

    let (peer, config) = start_peer(vec![(200, "", TERMINAL)]).await;
    let client = new_client(config.clone());

    let listings = client
        .get_listings_all(
            "POLYMARKET",
            &ListingsQuery {
                limit: 2,
                cursor: Some("cursor +/?=%".into()),
            },
            Pagination {
                max_pages: 1,
                max_records: 2,
            },
        )
        .await
        .unwrap();

    assert_eq!(listings.len(), 0);
    let requests = peer.await.unwrap();
    assert_request(
        &requests[0],
        "/v1/markets/POLYMARKET/history?limit=2&cursor=cursor+%2B%2F%3F%3D%25",
        &config,
    );
}

#[cfg(all(feature = "simulation", madsim))]
#[rstest]
fn retry_backoff_controls_forward_exact_delays() {
    for (initial, maximum, factor, immediate, millis) in [
        (10, 25, 2.0, false, 55),
        (7, 100, 1.0, false, 21),
        (10, 25, 2.0, true, 30),
    ] {
        let baseline =
            retry_runtime().block_on(retry_elapsed(initial, maximum, factor, immediate, 0, false));
        let retried =
            retry_runtime().block_on(retry_elapsed(initial, maximum, factor, immediate, 0, true));
        assert_eq!(
            (retried.checked_sub(baseline).unwrap().as_micros() + 500) / 1_000,
            millis
        );
    }
}

#[cfg(all(feature = "simulation", madsim))]
#[rstest]
fn retry_jitter_is_forwarded_under_fixed_seed() {
    let baseline = retry_runtime().block_on(retry_elapsed(10, 25, 2.0, false, 0, false));
    let jittered = retry_runtime().block_on(retry_elapsed(10, 25, 2.0, false, 1_000, true));
    assert_eq!(
        (jittered.checked_sub(baseline).unwrap().as_micros() + 500) / 1_000,
        75
    );
}

#[cfg(all(feature = "simulation", madsim))]
async fn retry_elapsed(
    initial: u64,
    maximum: u64,
    factor: f64,
    immediate: bool,
    jitter: u64,
    retry: bool,
) -> Duration {
    let (peer, mut config) = start_peer(vec![
        (503, "", EMPTY),
        (503, "", EMPTY),
        (503, "", EMPTY),
        (200, "", EXCHANGES),
    ])
    .await;

    config.retry.max_retries = if retry { 3 } else { 0 };
    config.retry.initial_delay_ms = initial;
    config.retry.max_delay_ms = maximum;
    config.retry.jitter_ms = jitter;
    config.retry.backoff_factor = factor;
    config.retry.immediate_first = immediate;
    let client = new_client(config);
    let start = time::Instant::now();

    if !retry {
        for _ in 0..3 {
            assert!(matches!(
                client.get_exchanges().await,
                Err(Error::Http { status: 503, .. })
            ));
        }
    }

    client.get_exchanges().await.unwrap();
    let elapsed = start.elapsed();
    assert_eq!(client.attempts_remaining(), 96);
    assert_eq!(peer.await.unwrap().len(), 4);
    elapsed
}

#[cfg(not(all(feature = "simulation", madsim)))]
#[tokio::test]
async fn environment_credentials_resolve_once_and_explicit_key_wins() {
    const CHILD: &str = "FINFEED_CREDENTIAL_TEST_CHILD";

    if std::env::var_os(CHILD).is_none() {
        for (mode, key) in [
            ("valid", Some("test-api-key")),
            ("missing", None),
            ("invalid", Some("invalid key")),
        ] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "dst::environment_credentials_resolve_once_and_explicit_key_wins",
                    "--nocapture",
                ])
                .env(CHILD, mode);

            match key {
                Some(key) => {
                    command.env("FINFEED_API_KEY", key);
                }
                None => {
                    command.env_remove("FINFEED_API_KEY");
                }
            }

            assert!(command.status().unwrap().success());
        }

        return;
    }

    let mode = std::env::var(CHILD).unwrap();
    if mode != "valid" {
        let e = FinFeedHttpClient::new(None, FinFeedHttpConfig::default()).unwrap_err();

        let expected = if mode == "missing" {
            "set api_key or FINFEED_API_KEY"
        } else {
            "API key must contain visible ASCII"
        };

        assert!(matches!(e,Error::Validation(message) if message==expected));
        return;
    }

    let (peer, config) = start_peer(vec![(200, "", EXCHANGES)]).await;
    let client = FinFeedHttpClient::new(None, config.clone()).unwrap();
    assert_eq!(client.get_exchanges().await.unwrap().len(), 9);
    assert_request(&peer.await.unwrap()[0], "/v1/exchanges", &config);
    let (peer, config) = start_peer(vec![(200, "", EXCHANGES)]).await;
    let client =
        FinFeedHttpClient::new(Some(SecretString::from("explicit-key")), config.clone()).unwrap();
    assert_eq!(client.get_exchanges().await.unwrap().len(), 9);
    let requests = peer.await.unwrap();
    let authorization: Vec<_> = requests[0]
        .lines()
        .filter(|line| line.starts_with("authorization:"))
        .collect();
    assert_eq!(authorization, vec!["authorization: explicit-key"]);
    assert_request(
        &requests[0].replace("explicit-key", "test-api-key"),
        "/v1/exchanges",
        &config,
    );
}

#[cfg(all(feature = "simulation", madsim))]
fn retry_runtime() -> madsim::runtime::Runtime {
    let mut config = madsim::Config::default();
    config.net.send_latency = Duration::ZERO..Duration::from_nanos(1);
    madsim::runtime::Runtime::with_seed_and_config(7, config)
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn dropped_request_releases_concurrency_for_next_request() {
    let (listener, address) = bind_peer().await;
    let (received, ready) = tokio::sync::oneshot::channel();

    let peer = task::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        read_request(&mut first).await;
        received.send(()).unwrap();
        let (mut second, _) = listener.accept().await.unwrap();
        read_request(&mut second).await;
        second
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{EXCHANGES}",
                    EXCHANGES.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        second.flush().await.unwrap();
    });

    let client = Arc::new(new_client(FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        min_request_interval_ms: 1,
        ..Default::default()
    }));

    let pending = task::spawn({
        let client = Arc::clone(&client);
        async move { client.get_exchanges().await }
    });

    ready.await.unwrap();
    pending.abort();
    assert_eq!(client.get_exchanges().await.unwrap().len(), 9);
    assert_eq!(client.attempts_remaining(), 98);
    peer.await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn instrument_startup_and_fresh_requests_return_exact_selections() {
    let (no_id, no_body, yes_id, yes_body) = binary_metadata_pair();
    let refreshed = metadata_with_tick(&yes_body, "0.01");
    let (peer, config) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", yes_body),
        (200, "", refreshed),
        (200, "", no_body),
    ])
    .await;
    let http = Arc::new(new_client(config.clone()));
    let mut provider = FinFeedInstrumentProvider::new(http);
    let yes = to_instrument_id("POLYMARKET", &yes_id).unwrap();
    let no = to_instrument_id("POLYMARKET", &no_id).unwrap();
    let selections = [FinFeedInstrumentSelection {
        exchange_id: "POLYMARKET".into(),
        market_ids: vec![yes_id.as_str().into()],
    }];
    let loaded = provider
        .load(&selections, UnixNanos::from(111))
        .await
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, yes);
    assert_eq!(loaded[0].outcome.unwrap().as_str(), "Yes");
    assert_eq!(loaded[0].ts_init, UnixNanos::from(111));
    assert_eq!(loaded[0].price_increment.to_string(), "0.0010");
    assert_eq!(provider.get(yes).unwrap().ts_init, UnixNanos::from(111));
    assert!(provider.get(no).is_none());

    let updated = provider
        .request_instrument(yes, UnixNanos::from(222))
        .await
        .unwrap();
    let requested_no = provider
        .request_instrument(no, UnixNanos::from(333))
        .await
        .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(updated.id, yes);
    assert_eq!(updated.price_increment.to_string(), "0.0100");
    assert_eq!(updated.ts_event, UnixNanos::from(222));
    assert_eq!(updated.ts_init, UnixNanos::from(222));
    assert_eq!(
        provider.get(yes).unwrap().price_increment,
        updated.price_increment
    );
    assert_eq!(provider.get(yes).unwrap().ts_init, updated.ts_init);
    assert_eq!(requested_no.id, no);
    assert_eq!(requested_no.outcome.unwrap().as_str(), "No");
    assert_eq!(provider.get(no).unwrap().ts_init, UnixNanos::from(333));
    assert_eq!(requests.len(), 4);
    for (request, target) in requests.iter().zip([
        "/v1/exchanges".to_owned(),
        format!("/v1/markets/POLYMARKET/{yes_id}"),
        format!("/v1/markets/POLYMARKET/{yes_id}"),
        format!("/v1/markets/POLYMARKET/{no_id}"),
    ]) {
        assert_request(request, &target, &config);
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn instrument_startup_failure_preserves_the_entire_previous_catalog() {
    let (no_id, no_body, yes_id, _) = binary_metadata_pair();
    let (peer, config) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", no_body.clone()),
        (200, "", EXCHANGES.to_owned()),
        (200, "", metadata_with_tick(&no_body, "0.01")),
        (200, "", no_body),
    ])
    .await;
    let http = Arc::new(new_client(config.clone()));
    let mut provider = FinFeedInstrumentProvider::new(http);
    let no = to_instrument_id("POLYMARKET", &no_id).unwrap();
    let yes = to_instrument_id("POLYMARKET", &yes_id).unwrap();
    provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec![no_id.as_str().into()],
            }],
            UnixNanos::from(11),
        )
        .await
        .unwrap();
    let e = provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec![no_id.as_str().into(), yes_id.as_str().into()],
            }],
            UnixNanos::from(22),
        )
        .await
        .unwrap_err();
    let requests = peer.await.unwrap();
    assert!(
        matches!(e, Error::Definition { exchange_id, market_id, field: "identity", reason: "does not match the requested outcome" } if exchange_id == "POLYMARKET" && market_id == yes_id)
    );
    assert_eq!(provider.get(no).unwrap().ts_init, UnixNanos::from(11));
    assert_eq!(
        provider.get(no).unwrap().price_increment.to_string(),
        "0.0010"
    );
    assert!(provider.get(yes).is_none());
    assert_eq!(requests.len(), 5);
    assert_request(
        &requests[4],
        &format!("/v1/markets/POLYMARKET/{yes_id}"),
        &config,
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn fresh_instrument_failures_preserve_prior_definition() {
    let (no_id, no_body, yes_id, yes_body) = binary_metadata_pair();
    let (peer, config) = start_peer(vec![
        (200, "", no_body.clone()),
        (200, "", yes_body),
        (404, "", String::new()),
        (200, "", metadata_with_tick(&no_body, "0.00001")),
    ])
    .await;
    let mut provider = FinFeedInstrumentProvider::new(Arc::new(new_client(config.clone())));
    let no = to_instrument_id("POLYMARKET", &no_id).unwrap();
    let yes = to_instrument_id("POLYMARKET", &yes_id).unwrap();
    provider
        .request_instrument(no, UnixNanos::from(1))
        .await
        .unwrap();
    let mismatch = provider
        .request_instrument(no, UnixNanos::from(2))
        .await
        .unwrap_err();
    let unavailable = provider
        .request_instrument(no, UnixNanos::from(3))
        .await
        .unwrap_err();
    let rounding = provider
        .request_instrument(no, UnixNanos::from(4))
        .await
        .unwrap_err();
    let requests = peer.await.unwrap();
    assert!(matches!(
        mismatch,
        Error::Definition {
            field: "identity",
            ..
        }
    ));
    assert!(matches!(unavailable, Error::Unavailable(id) if id == no));
    assert!(matches!(
        rounding,
        Error::Definition {
            field: "MinimumTickSize",
            reason: "would require rounding at price precision",
            ..
        }
    ));
    assert_eq!(provider.get(no).unwrap().ts_init, UnixNanos::from(1));
    assert_eq!(
        provider.get(no).unwrap().price_increment.to_string(),
        "0.0010"
    );
    assert!(provider.get(yes).is_none());
    assert_eq!(requests.len(), 4);
    for request in requests {
        assert_request(
            &request,
            &format!("/v1/markets/POLYMARKET/{no_id}"),
            &config,
        );
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn duplicate_startup_outcome_tokens_preserve_the_entire_previous_catalog() {
    let (no_id, no_body, yes_id, yes_body) = binary_metadata_pair();
    let alias_id = "ALIAS_NO";
    let mut alias: serde_json::Value = serde_json::from_str(&no_body).unwrap();
    alias["market_id"] = alias_id.into();
    let (peer, config) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", no_body.clone()),
        (200, "", yes_body),
        (200, "", EXCHANGES.to_owned()),
        (200, "", metadata_with_tick(&no_body, "0.01")),
        (200, "", alias.to_string()),
    ])
    .await;
    let mut provider = FinFeedInstrumentProvider::new(Arc::new(new_client(config.clone())));
    let no = to_instrument_id("POLYMARKET", &no_id).unwrap();
    let yes = to_instrument_id("POLYMARKET", &yes_id).unwrap();
    let alias = to_instrument_id("POLYMARKET", alias_id).unwrap();
    let previous = provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec![no_id.as_str().into(), yes_id.as_str().into()],
            }],
            UnixNanos::from(11),
        )
        .await
        .unwrap();
    let e = provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec![no_id.as_str().into(), alias_id.into()],
            }],
            UnixNanos::from(22),
        )
        .await
        .unwrap_err();
    let requests = peer.await.unwrap();

    assert!(
        matches!(e, Error::Validation(message) if message == format!("duplicate outcome mapping for {alias}"))
    );

    for (id, expected) in [no, yes].into_iter().zip(&previous) {
        assert_eq!(
            serde_json::to_value(provider.get(id).unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    assert!(provider.get(alias).is_none());
    assert_eq!(requests.len(), 6);
    for (request, target) in requests.iter().zip([
        "/v1/exchanges".to_owned(),
        format!("/v1/markets/POLYMARKET/{no_id}"),
        format!("/v1/markets/POLYMARKET/{yes_id}"),
        "/v1/exchanges".to_owned(),
        format!("/v1/markets/POLYMARKET/{no_id}"),
        format!("/v1/markets/POLYMARKET/{alias_id}"),
    ]) {
        assert_request(request, &target, &config);
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn different_provider_ids_cannot_claim_the_same_outcome_token() {
    let (no_id, no_body, _, _) = binary_metadata_pair();
    let alias_id = "ALIAS_NO";
    let mut alias: serde_json::Value = serde_json::from_str(&no_body).unwrap();
    alias["market_id"] = alias_id.into();
    let (peer, config) = start_peer(vec![(200, "", no_body), (200, "", alias.to_string())]).await;
    let mut provider = FinFeedInstrumentProvider::new(Arc::new(new_client(config.clone())));
    let no = to_instrument_id("POLYMARKET", &no_id).unwrap();
    let alias = to_instrument_id("POLYMARKET", alias_id).unwrap();
    provider
        .request_instrument(no, UnixNanos::from(1))
        .await
        .unwrap();
    let e = provider
        .request_instrument(alias, UnixNanos::from(2))
        .await
        .unwrap_err();
    let requests = peer.await.unwrap();
    assert!(
        matches!(e, Error::Validation(message) if message == format!("duplicate outcome mapping for {alias}"))
    );
    assert_eq!(provider.get(no).unwrap().ts_init, UnixNanos::from(1));
    assert!(provider.get(alias).is_none());
    assert_eq!(requests.len(), 2);
    assert_request(&requests[1], "/v1/markets/POLYMARKET/ALIAS_NO", &config);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn successful_startup_replaces_previous_selection() {
    let (no_id, no_body, yes_id, yes_body) = binary_metadata_pair();
    let (peer, config) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", no_body),
        (200, "", EXCHANGES.to_owned()),
        (200, "", yes_body),
    ])
    .await;
    let mut provider = FinFeedInstrumentProvider::new(Arc::new(new_client(config)));
    let no = to_instrument_id("POLYMARKET", &no_id).unwrap();
    let yes = to_instrument_id("POLYMARKET", &yes_id).unwrap();
    provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec![no_id.as_str().into()],
            }],
            UnixNanos::from(1),
        )
        .await
        .unwrap();
    let loaded = provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec![yes_id.as_str().into()],
            }],
            UnixNanos::from(2),
        )
        .await
        .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, yes);
    assert_eq!(provider.get(yes).unwrap().ts_init, UnixNanos::from(2));
    assert!(provider.get(no).is_none());
    assert_eq!(requests.len(), 4);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_selection_and_native_requests_consume_no_rest_attempts() {
    let http = Arc::new(new_client(FinFeedHttpConfig::default()));
    let remaining = http.attempts_remaining();
    let mut provider = FinFeedInstrumentProvider::new(http.clone());

    for selections in [
        vec![],
        vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![],
        }],
        vec![FinFeedInstrumentSelection {
            exchange_id: "UNKNOWN".into(),
            market_ids: vec!["market_NO".into()],
        }],
        vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec!["market_NO".into(), "market_NO".into()],
        }],
    ] {
        assert!(matches!(
            provider.load(&selections, UnixNanos::default()).await,
            Err(Error::Validation(_))
        ));
    }
    assert!(matches!(
        provider
            .request_instrument(
                "market_NO.POLYMARKET".parse().unwrap(),
                UnixNanos::default()
            )
            .await,
        Err(Error::Validation(_))
    ));
    assert_eq!(http.attempts_remaining(), remaining);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn selected_exchange_must_be_advertised_by_the_provider() {
    let exchanges: Vec<Exchange> = serde_json::from_str(EXCHANGES).unwrap();
    let polymarket = exchanges
        .into_iter()
        .find(|exchange| exchange.exchange_id.as_deref() == Some("POLYMARKET"))
        .unwrap();
    let body = serde_json::json!([{ "exchange_id": polymarket.exchange_id, "market_name_institution_description": polymarket.market_name_institution_description, "website": polymarket.website }]).to_string();
    let (peer, config) = start_peer(vec![(200, "", body)]).await;
    let mut provider = FinFeedInstrumentProvider::new(Arc::new(new_client(config.clone())));
    let e = provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "KALSHI".into(),
                market_ids: vec!["contract_NO".into()],
            }],
            UnixNanos::default(),
        )
        .await
        .unwrap_err();
    let requests = peer.await.unwrap();
    assert!(
        matches!(e, Error::Validation(message) if message == "selected exchange KALSHI is unavailable")
    );
    assert_eq!(requests.len(), 1);
    assert_request(&requests[0], "/v1/exchanges", &config);
}

fn binary_metadata_pair() -> (String, String, String, String) {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../../test_data/polymarket-listings.json")).unwrap();
    let no = value["data"][0].clone();
    let yes = value["data"][1].clone();
    (
        no["market_id"].as_str().unwrap().to_owned(),
        no.to_string(),
        yes["market_id"].as_str().unwrap().to_owned(),
        yes.to_string(),
    )
}

fn metadata_with_tick(body: &str, tick: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(body).unwrap();
    value["source_specific_data"]["MinimumTickSize"] = tick.into();
    value.to_string()
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn paginated_discovery_loads_only_the_explicit_outcome() {
    let (_, _, yes_id, yes_body) = binary_metadata_pair();
    let mut final_page: serde_json::Value = serde_json::from_str(include_str!(
        "../../test_data/polymarket-listings-next.json"
    ))
    .unwrap();
    final_page["next_cursor"] = serde_json::Value::Null;
    let active = serde_json::json!([yes_id]).to_string();
    let (peer, config) = start_peer(vec![
        (
            200,
            "",
            include_str!("../../test_data/polymarket-listings.json").to_owned(),
        ),
        (200, "", final_page.to_string()),
        (200, "", active),
        (
            200,
            "",
            include_str!("../../test_data/empty.json").to_owned(),
        ),
        (200, "", EXCHANGES.to_owned()),
        (200, "", yes_body),
    ])
    .await;
    let http = Arc::new(new_client(config.clone()));
    let bounds = Pagination {
        max_pages: 2,
        max_records: 4,
    };
    let listings = http
        .get_listings_all(
            "POLYMARKET",
            &ListingsQuery {
                limit: 2,
                cursor: None,
            },
            bounds,
        )
        .await
        .unwrap();
    let active = http
        .get_active_all(
            "POLYMARKET",
            &ActiveQuery { limit: 2, page: 1 },
            Pagination {
                max_pages: 2,
                max_records: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(active, vec![yes_id.clone()]);
    assert_eq!(listings.len(), 4);
    let expected_ids: Vec<_> = listings
        .iter()
        .map(|market| to_instrument_id("POLYMARKET", market.market_id.as_deref().unwrap()).unwrap())
        .collect();
    let selected = to_instrument_id("POLYMARKET", &yes_id).unwrap();
    let mut provider = FinFeedInstrumentProvider::new(http);
    let loaded = provider
        .load(
            &[FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: active,
            }],
            UnixNanos::from(7),
        )
        .await
        .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, selected);
    assert_eq!(loaded[0].outcome.unwrap().as_str(), "Yes");
    for id in expected_ids {
        assert_eq!(
            provider.get(id).map(|instrument| instrument.id),
            (id == selected).then_some(selected)
        );
    }
    assert_eq!(requests.len(), 6);

    for (request, target) in requests.iter().zip([
        "/v1/markets/POLYMARKET/history?limit=2".to_owned(),
        "/v1/markets/POLYMARKET/history?limit=2&cursor=test-pair_YES".to_owned(),
        "/v1/markets/POLYMARKET/active?limit=2&page=1".to_owned(),
        "/v1/markets/POLYMARKET/active?limit=2&page=2".to_owned(),
        "/v1/exchanges".to_owned(),
        format!("/v1/markets/POLYMARKET/{yes_id}"),
    ]) {
        assert_request(request, &target, &config);
    }
}
