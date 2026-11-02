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

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::Query,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::any,
};
use nautilus_binance::{
    common::{
        consts::{BINANCE_NAUTILUS_FUTURES_BROKER_ID, BINANCE_NAUTILUS_SPOT_BROKER_ID},
        encoder::encode_broker_id,
        enums::{BinanceEnvironment, BinanceProductType, BinanceSide, BinanceTimeInForce},
    },
    futures::http::{
        client::BinanceRawFuturesHttpClient,
        error::BinanceFuturesHttpError,
        query::{
            BatchCancelItem as FuturesBatchCancelItem, BinanceAlgoOrderQueryParams,
            BinanceCancelAllAlgoOrdersParams, BinanceCancelAllOrdersParams,
            BinanceCancelOrderParams, BinanceModifyOrderParams,
        },
    },
    spot::{
        enums::BinanceSpotOrderType,
        http::{
            client::BinanceRawSpotHttpClient, error::BinanceSpotHttpError, query::BatchCancelItem,
        },
        sbe::spot::{WriteBuf, error_response_codec::ErrorResponseEncoder},
    },
};
use nautilus_model::identifiers::ClientOrderId;
use nautilus_network::http::HttpClient;
use parking_lot::Mutex;
use rstest::rstest;

#[rstest]
#[case::spot(BinanceProductType::Spot)]
#[case::usd_m(BinanceProductType::UsdM)]
#[case::coin_m(BinanceProductType::CoinM)]
#[tokio::test]
async fn test_http_retries_only_transient_reads(
    #[case] product: BinanceProductType,
    #[values(false, true)] mutate: bool,
    #[values(400, 429, 503)] status: u16,
) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(
        move |method: Method, Query(query): Query<HashMap<String, String>>| {
            let captured = captured.clone();
            async move {
                let first = {
                    let mut requests = captured.lock();
                    requests.push((method, Instant::now(), query));
                    requests.len() == 1
                };

                if first {
                    // Synthetic transport failure followed by a canonical ping response
                    (
                        StatusCode::from_u16(status).unwrap(),
                        [("Retry-After", "2")],
                        "synthetic failure",
                    )
                        .into_response()
                } else {
                    (
                        StatusCode::OK,
                        include_str!("../../test_data/spot/http_json/ping_response.json"),
                    )
                        .into_response()
                }
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let succeeded = if product == BinanceProductType::Spot {
        let client = BinanceRawSpotHttpClient::new_with_json_responses(
            BinanceEnvironment::Live,
            Some("test-key".to_string()),
            Some("test-secret".to_string()),
            Some(url),
            Some(5000),
            Some(5),
            None,
            true,
        )
        .unwrap();

        if mutate {
            client
                .new_order(
                    "BTCUSDT",
                    BinanceSide::Buy,
                    BinanceSpotOrderType::Market,
                    None,
                    Some("1"),
                    None,
                    Some("retry-test"),
                    None,
                )
                .await
                .is_ok()
        } else {
            client.get_signed("ping", None::<&()>).await.is_ok()
        }
    } else {
        let client = BinanceRawFuturesHttpClient::new(
            product,
            BinanceEnvironment::Live,
            Some("test-key".to_string()),
            Some("test-secret".to_string()),
            Some(url),
            Some(5000),
            Some(5),
            None,
        )
        .unwrap();

        if mutate {
            client
                .post::<(), serde_json::Value>("order", None, None, true, true)
                .await
                .is_ok()
        } else {
            client
                .get::<(), serde_json::Value>("ping", None, true, false)
                .await
                .is_ok()
        }
    };
    server.abort();
    let requests = requests.lock();
    let expected_retry = !mutate && status != 400;
    assert_eq!(succeeded, expected_retry);
    assert_eq!(requests.len(), if expected_retry { 2 } else { 1 });
    let expected_method = if mutate { Method::POST } else { Method::GET };
    for (method, _, _) in requests.iter() {
        assert_eq!(*method, expected_method);
    }

    if expected_retry {
        assert!(requests[1].1.duration_since(requests[0].1) >= Duration::from_secs(2));
        let timestamp_first = requests[0].2["timestamp"].parse::<u64>().unwrap();
        let timestamp_retry = requests[1].2["timestamp"].parse::<u64>().unwrap();
        assert!(timestamp_retry > timestamp_first);
        assert_ne!(requests[0].2["signature"], requests[1].2["signature"]);
    }
}

#[rstest]
#[case::sbe_client(false)]
#[case::json_client(true)]
#[tokio::test]
async fn test_spot_public_json_read_retries(#[case] json_responses: bool) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(
        move |method: Method, headers: HeaderMap, Query(query): Query<HashMap<String, String>>| {
            let captured = captured.clone();
            async move {
                let first = {
                    let mut requests = captured.lock();
                    requests.push((
                        method,
                        query,
                        headers["accept"].to_str().unwrap().to_string(),
                    ));
                    requests.len() == 1
                };

                if first {
                    (StatusCode::SERVICE_UNAVAILABLE, "synthetic failure").into_response()
                } else {
                    (
                        StatusCode::OK,
                        include_str!("../../test_data/spot/http_json/ticker_price_response.json"),
                    )
                        .into_response()
                }
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BinanceRawSpotHttpClient::new_with_json_responses(
        BinanceEnvironment::Live,
        None,
        None,
        Some(url),
        None,
        Some(5),
        None,
        json_responses,
    )
    .unwrap();
    let result = client.ticker_price(Some("LTCBTC")).await;
    server.abort();
    let tickers = result.unwrap();

    assert_eq!(tickers.len(), 1);
    assert_eq!(tickers[0].symbol.as_str(), "LTCBTC");
    assert_eq!(tickers[0].price, "4.00000200");
    assert_eq!(
        *requests.lock(),
        vec![
            (
                Method::GET,
                HashMap::from([("symbol".to_string(), "LTCBTC".to_string())]),
                "application/json".to_string()
            );
            2
        ],
    );
}

#[derive(Clone)]
enum RawClient {
    Spot(BinanceRawSpotHttpClient),
    Futures(BinanceRawFuturesHttpClient),
}

impl RawClient {
    fn new(product: BinanceProductType, environment: BinanceEnvironment, url: String) -> Self {
        if product == BinanceProductType::Spot {
            Self::Spot(
                BinanceRawSpotHttpClient::new_with_json_responses(
                    environment,
                    Some("test-key".to_string()),
                    Some("test-secret".to_string()),
                    Some(url),
                    Some(5000),
                    Some(5),
                    None,
                    true,
                )
                .unwrap(),
            )
        } else {
            Self::Futures(
                BinanceRawFuturesHttpClient::new(
                    product,
                    environment,
                    Some("test-key".to_string()),
                    Some("test-secret".to_string()),
                    Some(url),
                    Some(5000),
                    Some(5),
                    None,
                )
                .unwrap(),
            )
        }
    }

    async fn trigger(&self) -> Result<(), String> {
        match self {
            Self::Spot(client) => client
                .post_signed("trigger", None::<&()>)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Self::Futures(client) => client
                .post::<(), serde_json::Value>("trigger", None, None, true, true)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
        }
    }

    async fn depth(&self, symbol: &str) -> Result<serde_json::Value, String> {
        let params = [("symbol", symbol)];

        match self {
            Self::Spot(client) => {
                let bytes = client
                    .get_signed("depth", Some(&params))
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())
            }
            Self::Futures(client) => client
                .get("depth", Some(&params), true, false)
                .await
                .map_err(|e| e.to_string()),
        }
    }

    async fn identity_commands(&self) -> Vec<Result<(), String>> {
        let id = ClientOrderId::from("O-20260922-160119-V2-000-8");

        match self {
            Self::Spot(client) => {
                let encoded = encode_broker_id(&id, BINANCE_NAUTILUS_SPOT_BROKER_ID);
                vec![
                    client
                        .cancel_order("BTCUSDT", None, Some(&encoded))
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_replace_order(
                            "BTCUSDT",
                            BinanceSide::Buy,
                            BinanceSpotOrderType::Limit,
                            Some(BinanceTimeInForce::Gtc),
                            Some("1"),
                            Some("100"),
                            None,
                            Some(&encoded),
                            Some("cancel-identity-test"),
                            Some(&encoded),
                        )
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                ]
            }
            Self::Futures(client) => {
                let encoded = encode_broker_id(&id, BINANCE_NAUTILUS_FUTURES_BROKER_ID);
                vec![
                    client
                        .cancel_order(&BinanceCancelOrderParams {
                            symbol: "BTCUSDT".into(),
                            order_id: None,
                            orig_client_order_id: Some(encoded.clone()),
                            recv_window: None,
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .batch_cancel_orders(&[FuturesBatchCancelItem::by_client_order_id(
                            "BTCUSDT",
                            encoded.clone(),
                        )])
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_algo_order(&BinanceAlgoOrderQueryParams {
                            algo_id: None,
                            client_algo_id: Some(encoded),
                            recv_window: None,
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                ]
            }
        }
    }

    async fn account_cancels(&self) -> Vec<Result<(), String>> {
        let id = ClientOrderId::from("O-20260922-160119-V2-000-8");

        match self {
            Self::Spot(client) => {
                let encoded = encode_broker_id(&id, BINANCE_NAUTILUS_SPOT_BROKER_ID);
                vec![
                    client
                        .cancel_order("BTCUSDT", Some(123), None)
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_order("BTCUSDT", None, Some(&encoded))
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .batch_cancel_orders(&[BatchCancelItem::by_order_id("BTCUSDT", 123)])
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_open_order_responses("BTCUSDT")
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                ]
            }
            Self::Futures(client) => {
                let encoded = encode_broker_id(&id, BINANCE_NAUTILUS_FUTURES_BROKER_ID);
                vec![
                    client
                        .cancel_order(&BinanceCancelOrderParams {
                            symbol: "BTCUSDT".into(),
                            order_id: Some(123),
                            orig_client_order_id: None,
                            recv_window: None,
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_order(&BinanceCancelOrderParams {
                            symbol: "BTCUSDT".into(),
                            order_id: None,
                            orig_client_order_id: Some(encoded.clone()),
                            recv_window: None,
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .batch_cancel_orders(&[FuturesBatchCancelItem::by_client_order_id(
                            "BTCUSDT",
                            encoded.clone(),
                        )])
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_all_orders(&BinanceCancelAllOrdersParams {
                            symbol: "BTCUSDT".into(),
                            recv_window: None,
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_algo_order(&BinanceAlgoOrderQueryParams {
                            algo_id: None,
                            client_algo_id: Some(encoded),
                            recv_window: None,
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    client
                        .cancel_all_algo_orders(&BinanceCancelAllAlgoOrdersParams {
                            symbol: "BTCUSDT".into(),
                            recv_window: None,
                        })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                ]
            }
        }
    }

    async fn account_modify(&self) -> Result<(), String> {
        let id = ClientOrderId::from("O-20260922-160119-V2-000-8");

        match self {
            Self::Spot(client) => {
                let encoded = encode_broker_id(&id, BINANCE_NAUTILUS_SPOT_BROKER_ID);
                client
                    .cancel_replace_order(
                        "BTCUSDT",
                        BinanceSide::Buy,
                        BinanceSpotOrderType::Limit,
                        Some(BinanceTimeInForce::Gtc),
                        Some("1"),
                        Some("100"),
                        None,
                        Some(&encoded),
                        Some("cancel-scope-test"),
                        Some(&encoded),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            Self::Futures(client) => {
                let encoded = encode_broker_id(&id, BINANCE_NAUTILUS_FUTURES_BROKER_ID);
                client
                    .modify_order(&BinanceModifyOrderParams {
                        symbol: "BTCUSDT".into(),
                        order_id: None,
                        orig_client_order_id: Some(encoded),
                        side: BinanceSide::Buy,
                        quantity: "1".into(),
                        price: "100".into(),
                        recv_window: None,
                    })
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        }
    }

    async fn blocked_writes(&self) -> [bool; 5] {
        match self {
            Self::Spot(client) => [
                matches!(
                    client.post_signed("order", None::<&()>).await,
                    Err(BinanceSpotHttpError::RateLimitBlocked)
                ),
                matches!(
                    client
                        .batch_cancel_orders(&[BatchCancelItem::by_order_id("BTCUSDT", 123)])
                        .await,
                    Err(BinanceSpotHttpError::RateLimitBlocked)
                ),
                matches!(
                    client.create_listen_key().await,
                    Err(BinanceSpotHttpError::RateLimitBlocked)
                ),
                matches!(
                    client.extend_listen_key("test-listen-key").await,
                    Err(BinanceSpotHttpError::RateLimitBlocked)
                ),
                matches!(
                    client.close_listen_key("test-listen-key").await,
                    Err(BinanceSpotHttpError::RateLimitBlocked)
                ),
            ],
            Self::Futures(client) => [
                matches!(
                    client
                        .post::<(), serde_json::Value>("order", None, None, true, true)
                        .await,
                    Err(BinanceFuturesHttpError::RateLimitBlocked)
                ),
                matches!(
                    client
                        .batch_request("batchOrders", &[("symbol", "BTCUSDT")], true)
                        .await,
                    Err(BinanceFuturesHttpError::RateLimitBlocked)
                ),
                matches!(
                    client.create_listen_key().await,
                    Err(BinanceFuturesHttpError::RateLimitBlocked)
                ),
                matches!(
                    client.keepalive_listen_key("test-listen-key").await,
                    Err(BinanceFuturesHttpError::RateLimitBlocked)
                ),
                matches!(
                    client.close_listen_key("test-listen-key").await,
                    Err(BinanceFuturesHttpError::RateLimitBlocked)
                ),
            ],
        }
    }

    async fn blocked_paths(&self) -> [bool; 6] {
        let read = match self {
            Self::Spot(client) => matches!(
                client.get_signed("depth", None::<&()>).await,
                Err(BinanceSpotHttpError::RateLimitBlocked)
            ),
            Self::Futures(client) => matches!(
                client
                    .get::<(), serde_json::Value>("depth", None, true, false)
                    .await,
                Err(BinanceFuturesHttpError::RateLimitBlocked)
            ),
        };

        let writes = self.blocked_writes().await;
        [read, writes[0], writes[1], writes[2], writes[3], writes[4]]
    }
}

#[rstest]
#[case::spot(BinanceProductType::Spot, BinanceProductType::Spot)]
#[case::usd_m(BinanceProductType::UsdM, BinanceProductType::UsdM)]
#[case::coin_m(BinanceProductType::CoinM, BinanceProductType::CoinM)]
#[case::usd_coin_m(BinanceProductType::UsdM, BinanceProductType::CoinM)]
#[tokio::test]
async fn test_http_backoff_shared_between_clients(
    #[case] product: BinanceProductType,
    #[case] other_product: BinanceProductType,
    #[values(429, 418)] status: u16,
    #[values(false, true)] recreate: bool,
) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(
        move |method: Method, Query(query): Query<HashMap<String, String>>| {
            let captured = captured.clone();
            async move {
                captured
                    .lock()
                    .push((method.clone(), Instant::now(), query));
                if method == Method::POST {
                    (
                        StatusCode::from_u16(status).unwrap(),
                        [("Retry-After", "1")],
                        "synthetic throttle",
                    )
                        .into_response()
                } else {
                    include_str!("../../test_data/spot/http_json/depth_response.json")
                        .into_response()
                }
            }
        },
    ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/{}",
        listener.local_addr().unwrap(),
        nautilus_core::UUID4::new()
    );

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = RawClient::new(product, BinanceEnvironment::Live, url.clone());
    let other =
        (!recreate).then(|| RawClient::new(other_product, BinanceEnvironment::Live, url.clone()));
    let first_result = client.trigger().await;
    drop(client);
    let other =
        other.unwrap_or_else(|| RawClient::new(other_product, BinanceEnvironment::Live, url));
    let results = tokio::join!(other.depth("BTCUSDT"), other.depth("ETHUSDT"));
    server.abort();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../test_data/spot/http_json/depth_response.json"
    ))
    .unwrap();
    let requests = requests.lock();
    let first_timestamp = requests[0].2["timestamp"].parse::<u64>().unwrap();

    assert!(first_result.is_err());
    assert_eq!(results.0.unwrap(), expected);
    assert_eq!(results.1.unwrap(), expected);
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].0, Method::POST);

    for (method, received, query) in &requests[1..] {
        assert_eq!(*method, Method::GET);
        assert!(*received >= requests[0].1 + Duration::from_secs(1));
        assert!(query["timestamp"].parse::<u64>().unwrap() >= first_timestamp + 1000);
    }

    let mut symbols = requests[1..]
        .iter()
        .map(|(_, _, query)| query["symbol"].as_str())
        .collect::<Vec<_>>();
    symbols.sort_unstable();
    assert_eq!(symbols, ["BTCUSDT", "ETHUSDT"]);
}

#[rstest]
#[case::ip(false, -1003)]
#[case::already_ip(true, -1003)]
#[tokio::test]
async fn test_http_command_identity_reads_fail_fast(
    #[values(
        BinanceProductType::Spot,
        BinanceProductType::UsdM,
        BinanceProductType::CoinM
    )]
    product: BinanceProductType,
    #[case] preblocked: bool,
    #[case] code: i64,
) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(move |method: Method| {
        let captured = captured.clone();
        async move {
            captured.lock().push(method);
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("Retry-After", "2")],
                serde_json::json!({"code": code, "msg": "identity throttle"}).to_string(),
            )
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/{}",
        listener.local_addr().unwrap(),
        nautilus_core::UUID4::new()
    );

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = RawClient::new(product, BinanceEnvironment::Live, url);

    if preblocked {
        assert!(client.trigger().await.is_err());
    }

    let results = tokio::time::timeout(Duration::from_millis(250), client.identity_commands())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    server.abort();
    let blocked = "Request not sent: Binance cooldown or local quota blocks admission; wait for capacity or expiry, or verify clearance and restart if expiry is unknown";
    let first_cause = if preblocked {
        blocked.to_string()
    } else {
        format!("Binance error {code} (HTTP 429): identity throttle")
    };

    let expected = if product == BinanceProductType::Spot {
        vec![
            Err(format!(
                "Validation error: Cancel identity lookup failed before submission: {first_cause}"
            )),
            Err(format!(
                "Validation error: Cancel-replace identity lookup failed before submission: {blocked}"
            )),
        ]
    } else {
        vec![
            Err(if preblocked {
                blocked.to_string()
            } else {
                format!("Order identity lookup failed before submission: {first_cause}")
            }),
            Err(blocked.to_string()),
            Err(blocked.to_string()),
        ]
    };

    assert_eq!(results, expected);
    assert_eq!(
        *requests.lock(),
        [if preblocked {
            Method::POST
        } else {
            Method::GET
        }]
    );
}

#[rstest]
#[case::spot_json(BinanceProductType::Spot, false)]
#[case::spot_sbe(BinanceProductType::Spot, true)]
#[case::usd_m(BinanceProductType::UsdM, false)]
#[case::coin_m(BinanceProductType::CoinM, false)]
#[tokio::test]
async fn test_http_account_429_does_not_block_market_data(
    #[case] product: BinanceProductType,
    #[case] sbe: bool,
) {
    let error_body = if sbe {
        let mut body = vec![0; 128];
        let mut encoder = ErrorResponseEncoder::default()
            .wrap(WriteBuf::new(&mut body), 8)
            .header(0)
            .parent()
            .unwrap();
        encoder.code(-1015);
        encoder.server_time(0);
        encoder.retry_after(0);
        encoder.msg("Too many new orders");
        encoder.data(&[]);
        let length = 8 + encoder.encoded_length();
        body.truncate(length);
        body
    } else {
        include_bytes!("../../test_data/spot/http_json/error_order_rate_limit.json").to_vec()
    };

    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(move |method: Method| {
        let captured = captured.clone();
        let error_body = error_body.clone();
        async move {
            captured.lock().push(method.clone());
            if method == Method::POST {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("Retry-After", "10")],
                    error_body,
                )
                    .into_response()
            } else {
                include_str!("../../test_data/spot/http_json/depth_response.json").into_response()
            }
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/{}",
        listener.local_addr().unwrap(),
        nautilus_core::UUID4::new()
    );

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = if sbe {
        RawClient::Spot(
            BinanceRawSpotHttpClient::new(
                BinanceEnvironment::Live,
                Some("test-key".into()),
                Some("test-secret".into()),
                Some(url),
                Some(5000),
                Some(5),
                None,
            )
            .unwrap(),
        )
    } else {
        RawClient::new(product, BinanceEnvironment::Live, url)
    };

    let first = client.trigger().await;
    let depth = tokio::time::timeout(Duration::from_secs(2), client.depth("BTCUSDT")).await;
    let order = tokio::time::timeout(Duration::from_millis(100), client.trigger()).await;
    server.abort();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../test_data/spot/http_json/depth_response.json"
    ))
    .unwrap();

    assert_eq!(
        first.unwrap_err(),
        "Binance error -1015 (HTTP 429): Too many new orders"
    );
    assert_eq!(depth.unwrap().unwrap(), expected);
    assert_eq!(
        order.unwrap().unwrap_err(),
        "Request not sent: Binance cooldown or local quota blocks admission; wait for capacity or expiry, or verify clearance and restart if expiry is unknown"
    );
    assert_eq!(*requests.lock(), [Method::POST, Method::GET]);
}

#[rstest]
#[case::spot(BinanceProductType::Spot, 4)]
#[case::usd_m(BinanceProductType::UsdM, 6)]
#[case::coin_m(BinanceProductType::CoinM, 6)]
#[tokio::test]
async fn test_http_order_cooldown_allows_cancellation(
    #[case] product: BinanceProductType,
    #[case] cancel_count: usize,
) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(move |method: Method, uri: Uri| {
        let captured = captured.clone();
        async move {
            captured
                .lock()
                .push((method.clone(), uri.path().to_string()));
            order_cooldown_response(product, method, &uri)
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/{}",
        listener.local_addr().unwrap(),
        nautilus_core::UUID4::new()
    );

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = RawClient::new(product, BinanceEnvironment::Live, url);
    let first = client.trigger().await;
    let cancels = tokio::time::timeout(Duration::from_secs(1), client.account_cancels())
        .await
        .unwrap();
    let modify = tokio::time::timeout(Duration::from_millis(250), client.account_modify())
        .await
        .unwrap();
    let submit = client.trigger().await;
    server.abort();
    let requests = requests.lock();
    let blocked = "Request not sent: Binance cooldown or local quota blocks admission; wait for capacity or expiry, or verify clearance and restart if expiry is unknown";

    assert_eq!(
        first.unwrap_err(),
        "Binance error -1015 (HTTP 429): Too many new orders"
    );
    assert_eq!(cancels, vec![Ok(()); cancel_count]);

    if product == BinanceProductType::CoinM {
        assert_eq!(modify, Ok(()));
    } else if product == BinanceProductType::Spot {
        assert_eq!(
            modify,
            Err(format!(
                "Validation error: Cancel-replace identity lookup failed before submission: {blocked}"
            ))
        );
    } else {
        assert_eq!(modify, Err(blocked.to_string()));
    }

    assert_eq!(submit, Err(blocked.to_string()));
    assert_eq!(
        requests
            .iter()
            .filter(|(method, _)| *method == Method::DELETE)
            .count(),
        cancel_count
    );
    assert_eq!(
        requests
            .iter()
            .filter(|(method, _)| *method == Method::POST)
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|(method, _)| *method == Method::PUT)
            .count(),
        usize::from(product == BinanceProductType::CoinM)
    );
}

fn order_cooldown_response(
    product: BinanceProductType,
    method: Method,
    uri: &Uri,
) -> axum::response::Response {
    if method == Method::POST {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", "5")],
            r#"{"code":-1015,"msg":"Too many new orders"}"#,
        )
            .into_response();
    }

    let endpoint = uri.path().rsplit('/').next().unwrap();

    let json = match endpoint {
        "algoOpenOrders" | "allOpenOrders" => serde_json::json!({"code":200,"msg":"done"}),
        "algoOrder" => match method {
            Method::GET => serde_json::from_str(include_str!(
                "../../test_data/futures/http_json/algo_order_response.json"
            ))
            .unwrap(),
            _ => {
                serde_json::json!({"algoId":42,"clientAlgoId":"cancel-algo","code":"200","msg":"success"})
            }
        },
        _ => {
            let response = match (product, method) {
                (BinanceProductType::Spot, Method::GET) => {
                    include_str!("../../test_data/spot/http_json/order_response.json")
                }
                (BinanceProductType::Spot, _) => {
                    include_str!("../../test_data/spot/http_json/cancel_order_response.json")
                }
                _ => include_str!("../../test_data/futures/http_json/order_response.json"),
            };

            let order: serde_json::Value = serde_json::from_str(response).unwrap();

            match endpoint {
                "batchOrders" | "openOrders" => serde_json::json!([order]),
                _ => order,
            }
        }
    };

    (StatusCode::OK, json.to_string()).into_response()
}

#[rstest]
#[case::spot(BinanceProductType::Spot)]
#[case::usd_m(BinanceProductType::UsdM)]
#[case::coin_m(BinanceProductType::CoinM)]
#[tokio::test]
async fn test_http_known_cooldown_blocks_write_paths(
    #[case] product: BinanceProductType,
    #[values(429, 418)] status: u16,
) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let app = Router::new().fallback(any(move |method: Method| {
        let captured = captured.clone();
        async move {
            captured.lock().push(method);
            (
                StatusCode::from_u16(status).unwrap(),
                [("Retry-After", "1")],
                "synthetic throttle",
            )
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/{}",
        listener.local_addr().unwrap(),
        nautilus_core::UUID4::new()
    );

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = RawClient::new(product, BinanceEnvironment::Live, url);
    let first = client.trigger().await;
    let blocked = tokio::time::timeout(Duration::from_millis(250), client.blocked_writes())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    server.abort();

    assert!(first.is_err());
    assert_eq!(blocked, [true; 5]);
    assert_eq!(*requests.lock(), [Method::POST]);
}

#[rstest]
#[case::spot(BinanceProductType::Spot)]
#[case::usd_m(BinanceProductType::UsdM)]
#[case::coin_m(BinanceProductType::CoinM)]
#[tokio::test]
async fn test_http_unknown_ban_blocks_all_send_paths(
    #[case] product: BinanceProductType,
    #[values(false, true)] decoded: bool,
    #[values(None, Some("invalid"))] retry_after: Option<&'static str>,
) {
    let requests = Arc::new(Mutex::new(0));
    let captured = requests.clone();

    let body = if decoded {
        include_str!("../../test_data/spot/http_json/error_ip_ban.json")
    } else {
        "banned"
    };

    let app = Router::new().fallback(any(move || {
        let captured = captured.clone();
        async move {
            *captured.lock() += 1;
            let mut headers = HeaderMap::new();
            if let Some(value) = retry_after {
                headers.insert("Retry-After", value.parse().unwrap());
            }

            (StatusCode::IM_A_TEAPOT, headers, body).into_response()
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/{}",
        listener.local_addr().unwrap(),
        nautilus_core::UUID4::new()
    );

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = RawClient::new(product, BinanceEnvironment::Live, url.clone());
    let first = client.depth("BTCUSDT").await.unwrap_err();
    drop(client);
    let recreated = RawClient::new(product, BinanceEnvironment::Live, url);
    let blocked = recreated.blocked_paths().await;
    server.abort();

    if decoded {
        assert_eq!(first, "Binance error -1003 (HTTP 418): IP banned");
    } else {
        let expected = match product {
            BinanceProductType::Spot => BinanceSpotHttpError::UnexpectedStatus {
                status: 418,
                body: "62616e6e6564".to_string(),
                retry_after: None,
            }
            .to_string(),
            _ => BinanceFuturesHttpError::UnexpectedStatus {
                status: 418,
                body: "banned".to_string(),
                retry_after: None,
            }
            .to_string(),
        };

        assert_eq!(first, expected);
    }

    assert_eq!(blocked, [true; 6]);
    assert_eq!(*requests.lock(), 1);
}

#[rstest]
#[case::spot(BinanceProductType::Spot)]
#[case::usd_m(BinanceProductType::UsdM)]
#[case::coin_m(BinanceProductType::CoinM)]
#[ignore = "Uses Binance Spot mainnet and Futures testnet public depth endpoints"]
#[tokio::test]
async fn test_http_backoff_live_venue_reads(
    #[case] product: BinanceProductType,
    #[values(429, 418)] status: u16,
) {
    let environment = if product == BinanceProductType::Spot {
        BinanceEnvironment::Live
    } else {
        BinanceEnvironment::Testnet
    };

    let symbol = if product == BinanceProductType::CoinM {
        "BTCUSD_PERP"
    } else {
        "BTCUSDT"
    };

    let path = match product {
        BinanceProductType::Spot => "/api/v3/depth",
        BinanceProductType::UsdM => "/fapi/v1/depth",
        BinanceProductType::CoinM => "/dapi/v1/depth",
        _ => unreachable!(),
    };

    let base = nautilus_binance::common::urls::get_http_base_url(product, environment);
    let target = format!("{base}{path}?symbol={symbol}&limit=5");
    let forwarded = Arc::new(Mutex::new(Vec::new()));
    let captured = forwarded.clone();
    let upstream = HttpClient::builder().timeout_secs(10).build().unwrap();

    let app = Router::new().fallback(any(move |method: Method| {
        let target = target.clone();
        let upstream = upstream.clone();
        let captured = captured.clone();
        async move {
            if method == Method::POST {
                return (
                    StatusCode::from_u16(status).unwrap(),
                    [("Retry-After", "1")],
                    "synthetic throttle",
                )
                    .into_response();
            }

            let sent = Instant::now();

            let response = match upstream.get(target, None, None, None, None).await {
                Ok(response) if response.status.is_success() => response,
                Ok(response) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        format!("Live upstream returned HTTP {}", response.status.as_u16()),
                    )
                        .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        format!("Live upstream request failed: {e}"),
                    )
                        .into_response();
                }
            };

            let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
            captured.lock().push((sent, body));
            (StatusCode::OK, response.body.to_vec()).into_response()
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/{}",
        listener.local_addr().unwrap(),
        nautilus_core::UUID4::new()
    );

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let first = RawClient::new(product, environment, url.clone());
    let start = Instant::now();
    let throttled = first.trigger().await;
    let second = RawClient::new(product, environment, url);
    let depth = second.depth(symbol).await.unwrap();
    server.abort();
    let forwarded = forwarded.lock();

    assert!(throttled.is_err());
    assert_eq!(forwarded.len(), 1);
    assert!(forwarded[0].0 >= start + Duration::from_secs(1));
    assert_eq!(depth, forwarded[0].1);
}
