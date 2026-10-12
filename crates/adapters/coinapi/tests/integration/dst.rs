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

use std::{collections::HashMap, num::NonZeroUsize, sync::Arc, time::Duration};

use jiff::{SignedDuration, Timestamp};
use nautilus_coinapi::{
    config::CoinApiDataClientConfig,
    http::{
        client::{CoinApiHttpClient, CoinApiHttpConfig},
        error::Error,
        query::{
            BookQuery, ExchangesQuery, HISTORY_LIMIT_MAX, HistoryQuery, HistoryRange, OhlcvQuery,
            SymbolsQuery, TradesQuery,
        },
    },
};
use nautilus_core::{UnixNanos, string::secret::SecretString};
use nautilus_model::{identifiers::InstrumentId, instruments::Instrument};
use nautilus_network::{
    dst::{net::TcpListener, task, time},
    retry::{RetryConfig, RetryError},
};
use rstest::rstest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SYMBOL: &str = "TEST_SPOT_BTC_USD";

fn config() -> CoinApiHttpConfig {
    CoinApiHttpConfig {
        base_url: "http://127.0.0.1:18991/v1/".into(),
        retry: RetryConfig {
            initial_delay_ms: 10,
            max_delay_ms: 100,
            jitter_ms: 0,
            max_retries: 2,
            operation_timeout_ms: Some(1_000),
            max_elapsed_ms: Some(5_000),
            ..RetryConfig::default()
        },
        ..CoinApiHttpConfig::default()
    }
}

fn client(config: CoinApiHttpConfig) -> CoinApiHttpClient {
    CoinApiHttpClient::new(SecretString::from("test-api-key"), config).unwrap()
}

fn range(limit: u32) -> HistoryQuery {
    HistoryQuery {
        time_start: "2026-01-01T00:00:00Z".parse().unwrap(),
        time_end: "2026-01-01T00:01:00Z".parse().unwrap(),
        limit,
    }
}

async fn peer(
    responses: Vec<(u16, &'static str, &'static str)>,
) -> (task::JoinHandle<Vec<String>>, CoinApiHttpConfig) {
    let (listener, address) = listener().await;

    let config = CoinApiHttpConfig {
        base_url: format!("http://{address}/v1/"),
        ..config()
    };

    let task = task::spawn(async move {
        let mut requests = Vec::new();

        for (status, headers, body) in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let byte = stream.read_u8().await.unwrap();
                request.push(byte);
            }

            requests.push(String::from_utf8(request).unwrap());
            let response = format!(
                "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
        }

        requests
    });

    (task, config)
}

fn assert_request(request: &str, target: &str, config: &CoinApiHttpConfig) {
    let mut lines = request.split("\r\n");
    assert_eq!(lines.next().unwrap(), format!("GET {target} HTTP/1.1"));
    let mut headers = lines.filter(|line| !line.is_empty()).collect::<Vec<_>>();
    headers.sort_unstable();
    let host = format!(
        "host: {}",
        config
            .base_url
            .trim_start_matches("http://")
            .trim_end_matches("/v1/")
    );
    assert_eq!(
        headers,
        vec![
            "accept: application/json",
            &host,
            "x-coinapi-key: test-api-key",
            "x-referrer: nautilus",
        ]
    );
    #[cfg(all(feature = "simulation", madsim))]
    assert_eq!(
        request,
        format!(
            "GET {target} HTTP/1.1\r\naccept: application/json\r\n{host}\r\nx-coinapi-key: test-api-key\r\nx-referrer: nautilus\r\n\r\n"
        )
    );
}

#[rstest]
#[case("exchanges")]
#[case("symbols")]
#[case("book")]
#[case("book_l3")]
#[case("quotes")]
#[case("trades")]
#[case("ohlcv")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn endpoint_wire_contract(#[case] family: &str) {
    let body = match family {
        "exchanges" => include_str!("../../test_data/exchanges.json"),
        "symbols" => include_str!("../../test_data/symbols.json"),
        "book" => include_str!("../../test_data/book.json"),
        "book_l3" => include_str!("../../test_data/book_l3.json"),
        "quotes" => include_str!("../../test_data/quotes.json"),
        "trades" => include_str!("../../test_data/trades.json"),
        _ => include_str!("../../test_data/ohlcv.json"),
    };

    let (peer, config) = peer(vec![(200, "X-Credits-Used: malformed\r\n", body)]).await;
    let client = client(config.clone());
    let history = "time_start=2026-01-01T00%3A00%3A00.0000000Z&time_end=2026-01-01T00%3A01%3A00.0000000Z&limit=2";

    let target = match family {
        "exchanges" => {
            assert_eq!(
                client
                    .get_exchanges(&ExchangesQuery {
                        filter_exchange_id: Some("TEST;OTHER".into())
                    })
                    .await
                    .unwrap(),
                serde_json::from_str::<Vec<_>>(body).unwrap()
            );
            "/v1/exchanges?filter_exchange_id=TEST%3BOTHER".into()
        }
        "symbols" => {
            assert_eq!(
                client
                    .get_symbols(
                        "TEST",
                        &SymbolsQuery {
                            filter_symbol_id: Some(SYMBOL.into()),
                            filter_asset_id: Some("BTC".into())
                        }
                    )
                    .await
                    .unwrap(),
                serde_json::from_str::<Vec<_>>(body).unwrap()
            );
            format!("/v1/symbols/TEST/active?filter_symbol_id={SYMBOL}&filter_asset_id=BTC")
        }
        "book" => {
            assert_eq!(
                client
                    .get_book(
                        SYMBOL,
                        &BookQuery {
                            limit_levels: Some(2)
                        }
                    )
                    .await
                    .unwrap(),
                serde_json::from_str(body).unwrap()
            );
            format!("/v1/orderbooks/{SYMBOL}/current?limit_levels=2")
        }
        "book_l3" => {
            assert_eq!(
                client
                    .get_book_l3(
                        SYMBOL,
                        &BookQuery {
                            limit_levels: Some(2)
                        }
                    )
                    .await
                    .unwrap(),
                serde_json::from_str(body).unwrap()
            );
            format!("/v1/orderbooks3/{SYMBOL}/current?limit_levels=2")
        }
        "quotes" => {
            assert_eq!(
                client.get_quotes(SYMBOL, &range(2)).await.unwrap(),
                serde_json::from_str::<Vec<_>>(body).unwrap()
            );
            format!("/v1/quotes/{SYMBOL}/history?{history}")
        }
        "trades" => {
            assert_eq!(
                client
                    .get_trades(
                        SYMBOL,
                        &TradesQuery {
                            range: range(2),
                            include_id: Some(false)
                        }
                    )
                    .await
                    .unwrap(),
                serde_json::from_str::<Vec<_>>(body).unwrap()
            );
            format!("/v1/trades/{SYMBOL}/history?{history}&include_id=false")
        }
        _ => {
            assert_eq!(
                client
                    .get_ohlcv(
                        SYMBOL,
                        &OhlcvQuery {
                            period_id: "1MIN".into(),
                            range: range(2)
                        }
                    )
                    .await
                    .unwrap(),
                serde_json::from_str::<Vec<_>>(body).unwrap()
            );
            format!("/v1/ohlcv/{SYMBOL}/history?period_id=1MIN&{history}")
        }
    };

    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request(&requests[0], &target, &config);
    assert_eq!(client.credits_remaining(), 99);
    assert!(!format!("{client:?}").contains("test-api-key"));
}

#[rstest]
#[case(403, 1)]
#[case(401, 1)]
#[case(400, 1)]
#[case(429, 3)]
#[case(503, 3)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn status_retry_contract(#[case] status: u16, #[case] attempts: usize) {
    let (peer, config) = peer(vec![
        (status, "", include_str!("../../test_data/error.json"));
        attempts
    ])
    .await;
    let client = client(config.clone());
    let error = client
        .get_exchanges(&ExchangesQuery::default())
        .await
        .unwrap_err();
    let requests = peer.await.unwrap();
    assert!(
        matches!(error, Error::Venue { status: actual, ref message, retry_after_secs: None } if actual == status && message == "permission denied")
    );
    assert_eq!(requests.len(), attempts);
    assert_eq!(client.credits_remaining(), 100 - attempts as u64);

    for request in requests {
        assert_request(&request, "/v1/exchanges", &config);
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn retry_after_then_success() {
    let (peer, config) = peer(vec![
        (
            429,
            "Retry-After: 1\r\n",
            include_str!("../../test_data/error.json"),
        ),
        (200, "", include_str!("../../test_data/empty.json")),
    ])
    .await;
    let client = client(config);
    let start = time::Instant::now();
    assert_eq!(
        client
            .get_exchanges(&ExchangesQuery::default())
            .await
            .unwrap(),
        vec![]
    );
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert_eq!(peer.await.unwrap().len(), 2);
    assert_eq!(client.credits_remaining(), 98);
}

#[rstest]
#[case(1, 1)]
#[case(100, 1)]
#[case(101, 2)]
#[case(100_000, 1_000)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn credit_admission(#[case] limit: u32, #[case] cost: u64) {
    let (peer, config) = peer(vec![(200, "", include_str!("../../test_data/empty.json"))]).await;

    let client = client(CoinApiHttpConfig {
        credit_budget: cost,
        ..config
    });

    assert_eq!(
        client.get_quotes(SYMBOL, &range(limit)).await.unwrap(),
        vec![]
    );
    assert!(matches!(
        client.get_quotes(SYMBOL, &range(limit)).await,
        Err(Error::CreditBudget)
    ));
    assert_eq!(peer.await.unwrap().len(), 1);

    // The empty page bills one credit and returns the rest of its reservation
    assert_eq!(client.credits_remaining(), cost - 1);
}

#[rstest]
#[case(0)]
#[case(100_001)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_limit_never_sends(#[case] limit: u32) {
    let client = client(config());
    assert!(matches!(
        client.get_quotes(SYMBOL, &range(limit)).await,
        Err(Error::Validation(_))
    ));
    assert_eq!(client.credits_remaining(), 100);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_never_sends() {
    let client = client(config());
    client.cancel();
    assert!(matches!(
        client.get_exchanges(&ExchangesQuery::default()).await,
        Err(Error::Retry(RetryError::Canceled))
    ));
    assert_eq!(client.credits_remaining(), 100);
}

#[rstest]
#[case(200)]
#[case(403)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn malformed_response_does_not_retry(#[case] status: u16) {
    let (peer, config) = peer(vec![(
        status,
        "",
        include_str!("../../test_data/malformed.txt"),
    )])
    .await;
    let client = client(config.clone());
    let error = client
        .get_exchanges(&ExchangesQuery::default())
        .await
        .unwrap_err();
    assert!(matches!(
        (status, error),
        (200, Error::Parse(_))
            | (
                403,
                Error::Http {
                    status: 403,
                    retry_after_secs: None
                }
            )
    ));
    assert_eq!(peer.await.unwrap().len(), 1);
    assert_eq!(client.credits_remaining(), 99);
}

#[cfg(all(feature = "simulation", madsim))]
async fn listener() -> (TcpListener, String) {
    (
        TcpListener::bind("127.0.0.1:18991").await.unwrap(),
        "127.0.0.1:18991".into(),
    )
}

#[cfg(not(all(feature = "simulation", madsim)))]
async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    (listener, address)
}

#[cfg(not(all(feature = "simulation", madsim)))]
#[tokio::test]
async fn data_config_routes_rest_through_explicit_proxy() {
    let (peer, proxy_config) =
        peer(vec![(200, "", include_str!("../../test_data/empty.json"))]).await;
    let proxy = proxy_config.base_url.trim_end_matches("/v1/").replacen(
        "http://",
        "http://user:secret@",
        1,
    );
    let config = CoinApiDataClientConfig::builder()
        .base_url_http("http://coinapi.invalid/v1/".into())
        .proxy_url(proxy.into())
        .max_retries(0)
        .build();
    let client = client(config.http_config());
    let exchanges = time::timeout(
        Duration::from_secs(3),
        client.get_exchanges(&ExchangesQuery::default()),
    )
    .await
    .unwrap()
    .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(exchanges, vec![]);
    assert_eq!(requests.len(), 1);
    let mut lines = requests[0].split("\r\n");
    assert_eq!(
        lines.next(),
        Some("GET http://coinapi.invalid/v1/exchanges HTTP/1.1")
    );
    let mut headers: Vec<_> = lines.filter(|line| !line.is_empty()).collect();
    headers.sort_unstable();
    assert_eq!(
        headers,
        vec![
            "accept: application/json",
            "host: coinapi.invalid",
            "proxy-authorization: Basic dXNlcjpzZWNyZXQ=",
            "x-coinapi-key: test-api-key",
            "x-referrer: nautilus",
        ]
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn data_config_allows_parallel_rest_requests() {
    let (listener, address) = listener().await;
    let config = CoinApiDataClientConfig::builder()
        .base_url_http(format!("http://{address}/v1/"))
        .max_concurrent_requests(2)
        .max_retries(0)
        .build();
    let client = Arc::new(client(config.http_config()));

    let first = task::spawn({
        let client = client.clone();
        async move { client.get_exchanges(&ExchangesQuery::default()).await }
    });

    let second = task::spawn({
        let client = client.clone();
        async move { client.get_exchanges(&ExchangesQuery::default()).await }
    });

    let mut streams = Vec::new();

    for _ in 0..2 {
        let (mut stream, _) = time::timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stream.read_u8().await.unwrap(), b'G');
        streams.push(stream);
    }

    for mut stream in streams {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]")
            .await
            .unwrap();
        stream.flush().await.unwrap();
    }

    assert_eq!(first.await.unwrap().unwrap(), vec![]);
    assert_eq!(second.await.unwrap().unwrap(), vec![]);
    assert_eq!(client.credits_remaining(), 98);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn concurrency_admission_and_cancellation_release_permits() {
    let (listener, address) = listener().await;

    let client = Arc::new(client(CoinApiHttpConfig {
        base_url: format!("http://{address}/v1/"),
        ..config()
    }));

    let first = task::spawn({
        let client = client.clone();
        async move { client.get_exchanges(&ExchangesQuery::default()).await }
    });

    let (mut first_stream, _) = listener.accept().await.unwrap();
    assert_eq!(first_stream.read_u8().await.unwrap(), b'G');

    let second = task::spawn({
        let client = client.clone();
        async move { client.get_exchanges(&ExchangesQuery::default()).await }
    });

    assert!(
        time::timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
    assert_eq!(client.credits_remaining(), 99);
    first.abort();
    assert!(first.await.is_err());
    let (mut second_stream, _) = time::timeout(Duration::from_secs(1), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        request.push(second_stream.read_u8().await.unwrap());
    }

    second_stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]")
        .await
        .unwrap();
    second_stream.flush().await.unwrap();
    assert_eq!(second.await.unwrap().unwrap(), vec![]);
    assert_eq!(client.credits_remaining(), 98);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_stops_inflight_and_queued_requests() {
    let (listener, address) = listener().await;

    let client = Arc::new(client(CoinApiHttpConfig {
        base_url: format!("http://{address}/v1/"),
        ..config()
    }));

    let first = task::spawn({
        let client = client.clone();
        async move { client.get_exchanges(&ExchangesQuery::default()).await }
    });

    let (mut stream, _) = listener.accept().await.unwrap();
    assert_eq!(stream.read_u8().await.unwrap(), b'G');

    let second = task::spawn({
        let client = client.clone();
        async move { client.get_exchanges(&ExchangesQuery::default()).await }
    });

    client.cancel();
    assert!(matches!(
        first.await.unwrap(),
        Err(Error::Retry(RetryError::Canceled))
    ));
    assert!(matches!(
        second.await.unwrap(),
        Err(Error::Retry(RetryError::Canceled))
    ));
    assert_eq!(client.credits_remaining(), 99);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn transport_disconnect_retries_then_recovers() {
    let (listener, address) = listener().await;

    let client = client(CoinApiHttpConfig {
        base_url: format!("http://{address}/v1/"),
        ..config()
    });

    let peer = task::spawn(async move {
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }

            if attempt == 1 {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]",
                    )
                    .await
                    .unwrap();
                stream.flush().await.unwrap();
            }
        }
    });

    assert_eq!(
        client
            .get_exchanges(&ExchangesQuery::default())
            .await
            .unwrap(),
        vec![]
    );
    peer.await.unwrap();
    assert_eq!(client.credits_remaining(), 98);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn request_timeout_is_bounded() {
    let (listener, address) = listener().await;
    let mut config = config();
    config.base_url = format!("http://{address}/v1/");
    config.retry.operation_timeout_ms = Some(20);
    config.retry.max_retries = 0;
    let client = client(config);

    let peer = task::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(stream.read_u8().await.unwrap(), b'G');
        time::sleep(Duration::from_secs(1)).await;
    });

    assert!(matches!(
        client.get_exchanges(&ExchangesQuery::default()).await,
        Err(Error::Retry(RetryError::OperationTimeout {
            timeout_ms: 20
        }))
    ));
    peer.abort();
    assert_eq!(client.credits_remaining(), 99);
}

#[rstest]
#[case("")]
#[case("key\r\nInjected: value")]
#[case(" example-secret")]
fn invalid_credentials_are_rejected_without_exposure(#[case] key: &str) {
    let error = CoinApiHttpClient::new(SecretString::from(key), config()).unwrap_err();
    assert!(matches!(error, Error::Validation(_)));

    if !key.is_empty() {
        assert!(!format!("{error:?}").contains(key));
    }
}

#[rstest]
#[case("file:///tmp/data")]
#[case("https://user:secret@example.com/v1/")]
#[case("https://example.com/v1/?key=secret")]
#[case("https://example.com/v1/#secret")]
fn invalid_endpoint_is_rejected(#[case] url: &str) {
    assert!(matches!(
        CoinApiHttpClient::new(
            SecretString::from("key"),
            CoinApiHttpConfig {
                base_url: url.into(),
                ..config()
            }
        ),
        Err(Error::Validation(_))
    ));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_parameters_do_not_consume_credits() {
    let client = client(config());
    let mut reversed = range(1);
    reversed.time_end = reversed.time_start;
    assert!(matches!(
        client.get_quotes(SYMBOL, &reversed).await,
        Err(Error::Validation(_))
    ));

    let sub_grid = HistoryQuery {
        time_start: "2026-01-01T00:00:00.000000001Z".parse().unwrap(),
        time_end: "2026-01-01T00:00:00.000000099Z".parse().unwrap(),
        limit: 1,
    };

    assert!(matches!(
        client.get_quotes(SYMBOL, &sub_grid).await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        client.get_symbols("..", &SymbolsQuery::default()).await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        client
            .get_book(
                SYMBOL,
                &BookQuery {
                    limit_levels: Some(0)
                }
            )
            .await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        client
            .get_ohlcv(
                SYMBOL,
                &OhlcvQuery {
                    period_id: String::new(),
                    range: range(1)
                }
            )
            .await,
        Err(Error::Validation(_))
    ));
    assert_eq!(client.credits_remaining(), 100);
}

fn at(micros: i64) -> Timestamp {
    "2026-01-01T00:00:00Z".parse::<Timestamp>().unwrap() + SignedDuration::from_micros(micros)
}

fn history_range(limit: Option<usize>) -> HistoryRange {
    HistoryRange {
        time_start: at(0),
        time_end: at(60_000_000),
        limit: limit.map(|limit| NonZeroUsize::new(limit).unwrap()),
    }
}

// Leaked so the peer can serve generated pages as static bodies
fn page(family: &str, times: &[Timestamp]) -> &'static str {
    let records: Vec<String> = times
        .iter()
        .map(|ts| match family {
            "quotes" => format!(
                r#"{{"symbol_id":"{SYMBOL}","time_exchange":"{ts}","time_coinapi":"{ts}","ask_price":2,"ask_size":1,"bid_price":1,"bid_size":1}}"#
            ),
            "trades" => format!(
                r#"{{"symbol_id":"{SYMBOL}","time_exchange":"{ts}","time_coinapi":"{ts}","uuid":"id","price":1,"size":1,"taker_side":"BUY"}}"#
            ),
            _ => format!(
                r#"{{"time_period_start":"{ts}","time_period_end":"{ts}","time_open":"{ts}","time_close":"{ts}","price_open":1,"price_high":1,"price_low":1,"price_close":1,"volume_traded":1,"trades_count":1}}"#
            ),
        })
        .collect();

    format!("[{}]", records.join(",")).leak()
}

fn history_target(family: &str, time_start: Timestamp, limit: usize) -> String {
    let period = if family == "ohlcv" {
        "period_id=1MIN&"
    } else {
        ""
    };

    let time_start = format!("{time_start:.7}").replace(':', "%3A");
    format!(
        "/v1/{family}/{SYMBOL}/history?{period}time_start={time_start}&time_end=2026-01-01T00%3A01%3A00.0000000Z&limit={limit}"
    )
}

// Returns the ordering timestamp of each record, which is what paging guarantees
async fn range_keys(
    client: &CoinApiHttpClient,
    family: &str,
    range: &HistoryRange,
) -> Result<(Vec<Timestamp>, bool), Error> {
    Ok(match family {
        "quotes" => {
            let pages = client.get_quotes_range(SYMBOL, range).await?;
            let keys = pages.records.iter().map(|q| q.time_exchange).collect();
            (keys, pages.is_truncated)
        }
        "trades" => {
            let pages = client.get_trades_range(SYMBOL, range).await?;
            let keys = pages.records.iter().map(|t| t.time_exchange).collect();
            (keys, pages.is_truncated)
        }
        _ => {
            let pages = client.get_ohlcv_range(SYMBOL, "1MIN", range).await?;
            let keys = pages.records.iter().map(|o| o.time_period_start).collect();
            (keys, pages.is_truncated)
        }
    })
}

#[rstest]
#[case("quotes", None, 100)]
#[case("trades", None, 100)]
#[case("ohlcv", None, 100)]
#[case("quotes", Some(HISTORY_LIMIT_MAX as usize + 5), 7)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_restarts_full_page_at_trailing_timestamp_group(
    #[case] family: &str,
    #[case] limit: Option<usize>,
    #[case] second_limit: usize,
) {
    let max = HISTORY_LIMIT_MAX as usize;
    let boundary = at(100 * (max as i64 - 2));
    let mut first: Vec<Timestamp> = (0..max - 1).map(|i| at(100 * i as i64)).collect();
    first.push(boundary);
    let second = vec![boundary, boundary, at(100 * max as i64)];

    let (peer, config) = peer(vec![
        (200, "", page(family, &first)),
        (200, "", page(family, &second)),
    ])
    .await;

    // One full page bills 1,000 credits, leaving one for a continuation of at most 100 records,
    // or of the 7 records a limit above the page size still allows.
    let client = client(CoinApiHttpConfig {
        credit_budget: 1_001,
        ..config.clone()
    });

    let (keys, is_truncated) = range_keys(&client, family, &history_range(limit))
        .await
        .unwrap();

    let mut expected = first[..max - 2].to_vec();
    expected.extend(&second);
    assert_eq!(keys, expected);
    assert!(!is_truncated);

    // Awaited after the keys so a pager that stops early fails instead of leaving the peer waiting
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_request(&requests[0], &history_target(family, at(0), max), &config);
    assert_request(
        &requests[1],
        &history_target(family, boundary, second_limit),
        &config,
    );
    assert_eq!(client.credits_remaining(), 0);
}

#[rstest]
#[case("quotes")]
#[case("trades")]
#[case("ohlcv")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_short_page_is_complete(#[case] family: &str) {
    let times = [at(0), at(1), at(1)];
    let (peer, config) = peer(vec![(200, "", page(family, &times))]).await;
    let client = client(config.clone());

    let (keys, is_truncated) = range_keys(&client, family, &history_range(None))
        .await
        .unwrap();

    let requests = peer.await.unwrap();
    assert_eq!(keys, times);
    assert!(!is_truncated);
    assert_eq!(requests.len(), 1);
    assert_request(
        &requests[0],
        &history_target(family, at(0), 10_000),
        &config,
    );
    assert_eq!(client.credits_remaining(), 99);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_full_page_at_limit_is_complete() {
    let times = [at(0), at(1), at(1)];
    let (peer, config) = peer(vec![(200, "", page("quotes", &times))]).await;
    let client = client(config.clone());

    let (keys, is_truncated) = range_keys(&client, "quotes", &history_range(Some(3)))
        .await
        .unwrap();

    let requests = peer.await.unwrap();
    assert_eq!(keys, times);
    assert!(!is_truncated);
    assert_eq!(requests.len(), 1);
    assert_request(&requests[0], &history_target("quotes", at(0), 3), &config);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_budget_truncates_before_trailing_timestamp_group() {
    let mut times: Vec<Timestamp> = (0..99).map(at).collect();
    times.push(at(98));
    let (peer, config) = peer(vec![(200, "", page("trades", &times))]).await;

    let client = client(CoinApiHttpConfig {
        credit_budget: 1,
        ..config.clone()
    });

    let (keys, is_truncated) = range_keys(&client, "trades", &history_range(None))
        .await
        .unwrap();

    let requests = peer.await.unwrap();
    assert_eq!(keys, times[..98]);
    assert!(is_truncated);
    assert_eq!(requests.len(), 1);
    assert_request(&requests[0], &history_target("trades", at(0), 100), &config);
    assert_eq!(client.credits_remaining(), 0);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_fails_when_one_timestamp_fills_page() {
    let (peer, config) = peer(vec![(200, "", page("trades", &[at(5); 100]))]).await;

    let client = client(CoinApiHttpConfig {
        credit_budget: 1,
        ..config
    });

    let error = range_keys(&client, "trades", &history_range(None))
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unexpected CoinAPI response: 100 records share timestamp 2026-01-01T00:00:00.000005Z, so paging cannot advance"
    );
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_rejects_descending_page() {
    let (peer, config) = peer(vec![(200, "", page("quotes", &[at(2), at(1)]))]).await;
    let client = client(config);

    let error = range_keys(&client, "quotes", &history_range(None))
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unexpected CoinAPI response: history page is not in ascending time order"
    );
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_drops_periods_outside_window() {
    let start = at(0);
    let end = at(60_000_000);
    let times = [
        start - SignedDuration::from_secs(60),
        start,
        end - SignedDuration::from_secs(60),
        end,
    ];
    let (peer, config) = peer(vec![(200, "", page("ohlcv", &times))]).await;
    let client = client(config);

    let (keys, is_truncated) = range_keys(&client, "ohlcv", &history_range(None))
        .await
        .unwrap();

    assert_eq!(keys, times[1..3]);
    assert!(!is_truncated);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_full_page_outside_window_is_empty() {
    let before: Vec<Timestamp> = (0..100).map(|i| at(-60_000_000 + i)).collect();
    let (peer, config) = peer(vec![(200, "", page("ohlcv", &before))]).await;

    let client = client(CoinApiHttpConfig {
        credit_budget: 1,
        ..config
    });

    let (keys, is_truncated) = range_keys(&client, "ohlcv", &history_range(None))
        .await
        .unwrap();

    assert_eq!(keys, vec![]);
    assert!(!is_truncated);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn concurrent_ranges_size_pages_after_earlier_ranges_settle() {
    let times = [at(0), at(1), at(2)];
    let body = page("quotes", &times);
    let (peer, config) = peer(vec![(200, "", body), (200, "", body)]).await;
    let client = client(config.clone());
    let range = history_range(None);

    let (first, second) = tokio::join!(
        range_keys(&client, "quotes", &range),
        range_keys(&client, "quotes", &range),
    );

    // Each page bills one credit, so the second range sizes its page from 99
    assert_eq!(first.unwrap(), (times.to_vec(), false));
    assert_eq!(second.unwrap(), (times.to_vec(), false));
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_request(
        &requests[0],
        &history_target("quotes", at(0), 10_000),
        &config,
    );
    assert_request(
        &requests[1],
        &history_target("quotes", at(0), 9_900),
        &config,
    );
    assert_eq!(client.credits_remaining(), 98);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_retry_exhausting_credits_returns_truncated_pages() {
    let max = HISTORY_LIMIT_MAX as usize;
    let boundary = at(100 * (max as i64 - 2));
    let mut first: Vec<Timestamp> = (0..max - 1).map(|i| at(100 * i as i64)).collect();
    first.push(boundary);

    let (peer, config) = peer(vec![
        (200, "", page("trades", &first)),
        (429, "", include_str!("../../test_data/error.json")),
    ])
    .await;

    // The failed continuation keeps its one-credit reservation, so its retry cannot be admitted
    let client = client(CoinApiHttpConfig {
        credit_budget: 1_001,
        ..config.clone()
    });

    let (keys, is_truncated) = range_keys(&client, "trades", &history_range(None))
        .await
        .unwrap();

    assert_eq!(keys, first[..max - 2]);
    assert!(is_truncated);
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_request(
        &requests[1],
        &history_target("trades", boundary, 100),
        &config,
    );
    assert_eq!(client.credits_remaining(), 0);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_without_credits_never_sends() {
    let (peer, config) = peer(vec![(200, "", include_str!("../../test_data/empty.json"))]).await;

    let client = client(CoinApiHttpConfig {
        credit_budget: 1,
        ..config
    });

    client.get_quotes(SYMBOL, &range(1)).await.unwrap();

    let result = range_keys(&client, "quotes", &history_range(None)).await;

    assert!(matches!(result, Err(Error::CreditBudget)));
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case("quotes")]
#[case("trades")]
#[case("ohlcv")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn range_between_grid_points_is_empty_without_request(#[case] family: &str) {
    let (peer, config) = peer(vec![]).await;
    let client = client(config);

    let range = HistoryRange {
        time_start: "2026-01-01T00:00:00.000000001Z".parse().unwrap(),
        time_end: "2026-01-01T00:00:00.000000099Z".parse().unwrap(),
        limit: None,
    };

    let result = range_keys(&client, family, &range).await.unwrap();

    assert_eq!(result, (Vec::new(), false));
    assert_eq!(client.credits_remaining(), 100);
    assert!(peer.await.unwrap().is_empty());
}

#[rstest]
#[case("quotes")]
#[case("trades")]
#[case("ohlcv")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn successful_page_settles_to_returned_records(#[case] family: &str) {
    let times: Vec<Timestamp> = (0..150).map(at).collect();
    let (peer, config) = peer(vec![(200, "", page(family, &times))]).await;
    let client = client(config);

    // Reserves 10 credits for the 1,000-record page, then bills 2 for 150 records
    let (keys, _) = range_keys(&client, family, &history_range(Some(1_000)))
        .await
        .unwrap();

    assert_eq!(keys, times);
    assert_eq!(peer.await.unwrap().len(), 1);
    assert_eq!(client.credits_remaining(), 98);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn retry_cannot_exceed_credit_budget() {
    let (peer, config) = peer(vec![(429, "", include_str!("../../test_data/error.json"))]).await;

    let client = client(CoinApiHttpConfig {
        credit_budget: 1,
        ..config
    });

    assert!(matches!(
        client.get_exchanges(&ExchangesQuery::default()).await,
        Err(Error::CreditBudget)
    ));
    assert_eq!(peer.await.unwrap().len(), 1);
    assert_eq!(client.credits_remaining(), 0);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn backoff_cannot_exceed_elapsed_budget() {
    let (peer, mut config) = peer(vec![(
        429,
        "Retry-After: 10\r\n",
        include_str!("../../test_data/error.json"),
    )])
    .await;
    config.retry.max_elapsed_ms = Some(500);
    let client = client(config);
    let start = time::Instant::now();
    assert!(matches!(
        client.get_exchanges(&ExchangesQuery::default()).await,
        Err(Error::Venue {
            status: 429,
            retry_after_secs: Some(10),
            ..
        })
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(peer.await.unwrap().len(), 1);
    assert_eq!(client.credits_remaining(), 99);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn path_segments_are_encoded_and_optional_queries_are_omitted() {
    let (peer, mut config) =
        peer(vec![(200, "", include_str!("../../test_data/empty.json"))]).await;
    config.base_url.pop();
    let client = client(config.clone());
    assert_eq!(
        client
            .get_trades(
                "TEST/BTC?USD",
                &TradesQuery {
                    range: range(1),
                    include_id: None
                }
            )
            .await
            .unwrap(),
        vec![]
    );
    let request = peer.await.unwrap().pop().unwrap();
    assert!(request.starts_with("GET /v1/trades/TEST%2FBTC%3FUSD/history?time_start=2026-01-01T00%3A00%3A00.0000000Z&time_end=2026-01-01T00%3A01%3A00.0000000Z&limit=1 HTTP/1.1\r\n"));
    assert!(!request.contains("include_id"));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn redirects_do_not_forward_credentials() {
    let (peer, config) = peer(vec![(
        302,
        "Location: http://127.0.0.1:18992/secret\r\n",
        "",
    )])
    .await;
    let client = client(config);
    let error = client
        .get_exchanges(&ExchangesQuery::default())
        .await
        .unwrap_err();
    assert!(!error.is_retryable());
    assert!(!format!("{error:?}").contains("test-api-key"));
    assert_eq!(peer.await.unwrap().len(), 1);
    assert_eq!(client.credits_remaining(), 99);
}

#[rstest]
#[case(0, 1, Some(1))]
#[case(1, 0, Some(1))]
#[case(1, 1, None)]
#[case(1, 1, Some(0))]
fn admission_configuration_requires_positive_bounds(
    #[case] concurrency: usize,
    #[case] budget: u64,
    #[case] elapsed: Option<u64>,
) {
    let mut config = config();
    config.max_concurrent_requests = concurrency;
    config.credit_budget = budget;
    config.retry.max_elapsed_ms = elapsed;
    assert!(matches!(
        CoinApiHttpClient::new(SecretString::from("test-api-key"), config),
        Err(Error::Validation(_))
    ));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn venue_error_redacts_credentials() {
    let (peer, config) = peer(vec![(
        403,
        "",
        include_str!("../../test_data/error_credential.json"),
    )])
    .await;
    let client = client(config);
    let error = client
        .get_exchanges(&ExchangesQuery::default())
        .await
        .unwrap_err();
    assert!(
        matches!(&error, Error::Venue { status: 403, message, retry_after_secs: None } if message == "Invalid key <redacted>")
    );
    assert_eq!(
        error.to_string(),
        "CoinAPI HTTP status 403: Invalid key <redacted>"
    );
    assert!(!format!("{error:?}").contains("test-api-key"));
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn instrument_loading_wire_contract() {
    let body = include_str!("../../test_data/symbols_futures.json");
    let (peer, config) = peer(vec![(200, "", body)]).await;
    let client = client(config.clone());

    let instruments = client
        .request_instruments(
            "TEST",
            &SymbolsQuery {
                filter_symbol_id: Some("TEST_FTS_BTC".into()),
                filter_asset_id: None,
            },
            &HashMap::new(),
            UnixNanos::from(1),
        )
        .await
        .unwrap();

    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request(
        &requests[0],
        "/v1/symbols/TEST/active?filter_symbol_id=TEST_FTS_BTC",
        &config,
    );
    assert_eq!(
        instruments
            .iter()
            .map(|parsed| (
                parsed.instrument.id().to_string(),
                parsed.symbol_id.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "BTCUSDT_261225.BINANCE".to_string(),
                "BINANCEFTS_FTS_BTC_USDT_261225"
            ),
            (
                "BTCUSDT-30OCT26-LINEAR.BYBIT".to_string(),
                "BYBIT_FTS_BTC_USDT_261030"
            ),
            (
                "BTCUSDZ26-INVERSE.BYBIT".to_string(),
                "BYBITINVERSE_FTS_BTC_USD_261225"
            ),
            ("BTC-USD-261225.OKX".to_string(), "OKEX_FTS_BTC_USD_261225"),
        ]
    );
    assert_eq!(client.credits_remaining(), 99);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn instrument_loading_skips_unmapped_symbols() {
    let body = include_str!("../../test_data/symbols_rejected.json");
    let (peer, config) = peer(vec![(200, "", body)]).await;
    let client = client(config);

    let instruments = client
        .request_instruments(
            "TEST",
            &SymbolsQuery::default(),
            &HashMap::new(),
            UnixNanos::from(1),
        )
        .await
        .unwrap();

    assert_eq!(peer.await.unwrap().len(), 1);
    assert_eq!(instruments, vec![]);
    assert_eq!(client.credits_remaining(), 99);
}

// A precision beyond the Nautilus maximum is a caller error, rejected before any credit is spent
#[rstest]
#[case(19)]
#[case(29)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn instrument_loading_rejects_invalid_price_precision(#[case] precision: u8) {
    let (peer, config) = peer(vec![]).await;
    let client = client(config);

    let error = client
        .request_instruments(
            "TEST",
            &SymbolsQuery::default(),
            &HashMap::from([(InstrumentId::from("BTC-USD.COINBASE"), precision)]),
            UnixNanos::from(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(&error, Error::Validation(message) if message.starts_with("price precision for BTC-USD.COINBASE")),
        "unexpected {error:?}"
    );
    assert!(peer.await.unwrap().is_empty());
    assert_eq!(client.credits_remaining(), 100);
}

mod websocket;
