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
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::any,
};
use nautilus_binance::{
    common::enums::{BinanceEnvironment, BinanceProductType, BinanceSide},
    futures::http::client::BinanceRawFuturesHttpClient,
    spot::{enums::BinanceSpotOrderType, http::client::BinanceRawSpotHttpClient},
};
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
