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

//! Integration tests for the Ax HTTP client using a mock Axum server.

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, time::Duration};

use axum::{Router, http::StatusCode, response::Json, routing::get};
use nautilus_architect_ax::http::{
    client::{AxHttpClient, AxRawHttpClient},
    error::AxHttpError,
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::http::HttpClient;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

/// Wait for the test server to be ready by polling a health endpoint.
async fn wait_for_server(addr: SocketAddr, path: &str) {
    let health_url = format!("http://{addr}{path}");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

fn create_router() -> Router {
    Router::new()
        .route(
            "/instruments",
            get(|| async { Json(load_test_data("http_get_instruments.json")) }),
        )
        .route(
            "/instrument",
            get(|| async {
                let data = load_test_data("http_get_instruments.json");
                let instruments = data["instruments"].as_array().unwrap();
                Json(instruments[0].clone())
            }),
        )
        .route(
            "/balances",
            get(|| async { Json(load_test_data("http_get_balances.json")) }),
        )
        .route(
            "/positions",
            get(|| async { Json(load_test_data("http_get_positions.json")) }),
        )
        .route(
            "/whoami",
            get(|| async { Json(load_test_data("http_get_whoami.json")) }),
        )
        .route(
            "/tickers",
            get(|| async {
                Json(json!({
                    "tickers": [
                        {
                            "symbol": "BTC-PERP",
                            "bid": "45000.00",
                            "ask": "45001.00",
                            "last": "45000.50",
                            "mark": "45000.25",
                            "volume_24h": "1000000.00"
                        }
                    ]
                }))
            }),
        )
        .route(
            "/ticker",
            get(|| async {
                Json(json!({
                    "symbol": "BTC-PERP",
                    "bid": "45000.00",
                    "ask": "45001.00",
                    "last": "45000.50",
                    "mark": "45000.25",
                    "volume_24h": "1000000.00"
                }))
            }),
        )
}

async fn start_test_server() -> SocketAddr {
    let router = create_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/instruments").await;
    addr
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_instruments_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client =
        AxRawHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    let response = client.get_instruments().await.unwrap();

    assert_eq!(response.instruments.len(), 3);
    assert_eq!(response.instruments[0].symbol.as_str(), "BTC-PERP");
    assert_eq!(response.instruments[1].symbol.as_str(), "ETH-PERP");
    assert_eq!(response.instruments[2].symbol.as_str(), "SOL-PERP");
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_instrument_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client =
        AxRawHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    let instrument = client.get_instrument("BTC-PERP").await.unwrap();

    assert_eq!(instrument.symbol.as_str(), "BTC-PERP");
    assert_eq!(instrument.tick_size, dec!(0.5));
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_balances_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        Some(60),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".to_string());

    let response = client.get_balances().await.unwrap();

    assert!(!response.balances.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_positions_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        Some(60),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".to_string());

    let response = client.get_positions().await.unwrap();

    assert!(!response.positions.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_tickers_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        Some(60),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".to_string());

    let response = client.get_tickers().await.unwrap();

    assert!(!response.tickers.is_empty());
    assert_eq!(response.tickers[0].symbol.as_str(), "BTC-PERP");
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_ticker_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        Some(60),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".to_string());

    let ticker = client.get_ticker("BTC-PERP").await.unwrap();

    assert_eq!(ticker.symbol.as_str(), "BTC-PERP");
    assert_eq!(ticker.bid, Some(dec!(45000.00)));
    assert_eq!(ticker.ask, Some(dec!(45001.00)));
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_instruments_returns_nautilus_types() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    let instruments = client
        .request_instruments(Some(Decimal::new(2, 4)), Some(Decimal::new(5, 4)))
        .await
        .unwrap();

    // Should have 2 instruments (SOL-PERP is suspended and skipped)
    assert_eq!(instruments.len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_instrument_returns_nautilus_type() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    let instrument = client
        .request_instrument("BTC-PERP", None, None)
        .await
        .unwrap();

    match instrument {
        InstrumentAny::CryptoPerpetual(perp) => {
            assert_eq!(perp.id.symbol.as_str(), "BTC-PERP");
            assert_eq!(perp.id.venue.as_str(), "AX");
        }
        _ => panic!("Expected CryptoPerpetual instrument"),
    }
}

#[rstest]
#[tokio::test]
async fn test_domain_http_cache_instruments() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    assert!(!client.is_initialized());

    let instruments = client.request_instruments(None, None).await.unwrap();
    client.cache_instruments(instruments);

    assert!(client.is_initialized());

    let cached_symbols = client.get_cached_symbols();
    assert_eq!(cached_symbols.len(), 2);
    assert!(cached_symbols.contains(&"BTC-PERP".to_string()));
    assert!(cached_symbols.contains(&"ETH-PERP".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_domain_http_get_cached_instrument() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    let instruments = client.request_instruments(None, None).await.unwrap();
    client.cache_instruments(instruments);

    let btc_symbol = ustr::Ustr::from("BTC-PERP");
    let cached = client.get_instrument(&btc_symbol);
    assert!(cached.is_some());

    let eth_symbol = ustr::Ustr::from("ETH-PERP");
    let cached = client.get_instrument(&eth_symbol);
    assert!(cached.is_some());

    let unknown_symbol = ustr::Ustr::from("UNKNOWN-PERP");
    let cached = client.get_instrument(&unknown_symbol);
    assert!(cached.is_none());
}

// Error handling tests

#[rstest]
#[tokio::test]
async fn test_http_network_error_invalid_port() {
    let base_url = "http://127.0.0.1:1".to_string();

    let client =
        AxRawHttpClient::new(Some(base_url), None, Some(1), Some(0), None, None, None).unwrap();

    let result = client.get_instruments().await;

    assert!(result.is_err());
    match result {
        Err(AxHttpError::NetworkError(_)) => {}
        other => panic!("expected NetworkError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_500_internal_server_error() {
    let router = Router::new().route(
        "/instruments",
        get(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Internal server error"
                })),
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/instruments").await;

    let base_url = format!("http://{addr}");
    let client =
        AxRawHttpClient::new(Some(base_url), None, Some(60), Some(0), None, None, None).unwrap();

    let result = client.get_instruments().await;

    assert!(result.is_err());
    match result {
        Err(AxHttpError::UnexpectedStatus { status, .. }) => {
            assert_eq!(status, 500);
        }
        other => panic!("expected UnexpectedStatus: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_malformed_json_response() {
    let router = Router::new().route("/instruments", get(|| async { "not valid json" }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/instruments").await;

    let base_url = format!("http://{addr}");
    let client =
        AxRawHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    let result = client.get_instruments().await;

    assert!(result.is_err());
    match result {
        Err(AxHttpError::JsonError(_)) => {}
        other => panic!("expected JsonError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_empty_instruments_response() {
    let router = Router::new().route(
        "/instruments",
        get(|| async {
            Json(json!({
                "instruments": []
            }))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/instruments").await;

    let base_url = format!("http://{addr}");
    let client =
        AxRawHttpClient::new(Some(base_url), None, Some(60), None, None, None, None).unwrap();

    let result = client.get_instruments().await.unwrap();

    assert!(result.instruments.is_empty());
}
