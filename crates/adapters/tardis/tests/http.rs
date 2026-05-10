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

//! Integration tests for the Tardis HTTP client using a mock Axum server.

use std::net::SocketAddr;

use ahash::AHashSet;
use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
use nautilus_tardis::{
    common::enums::TardisExchange,
    http::{TardisHttpClient, error::Error},
};
use rstest::rstest;

const SPOT_FIXTURE: &str = include_str!("../test_data/instrument_spot.json");
const PERPETUAL_FIXTURE: &str = include_str!("../test_data/instrument_perpetual.json");

async fn start_mock_server(app: Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

fn create_client(addr: SocketAddr) -> TardisHttpClient {
    let base_url = format!("http://{addr}");
    TardisHttpClient::new(Some("test_key"), Some(&base_url), Some(5), true, None).unwrap()
}

#[rstest]
#[tokio::test]
async fn test_instruments_info_list_response() {
    let app = Router::new().route(
        "/instruments/{exchange}",
        get(|| async { format!("[{SPOT_FIXTURE}]") }),
    );
    let (addr, _handle) = start_mock_server(app).await;
    let client = create_client(addr);

    let result = client
        .instruments_info(TardisExchange::Deribit, None, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id.as_str(), "BTC_USDC");
    assert_eq!(result[0].exchange, TardisExchange::Deribit);
}

#[rstest]
#[tokio::test]
async fn test_instruments_info_single_response() {
    let app = Router::new().route(
        "/instruments/{exchange}/{symbol}",
        get(|| async { SPOT_FIXTURE.to_string() }),
    );
    let (addr, _handle) = start_mock_server(app).await;
    let client = create_client(addr);

    let result = client
        .instruments_info(TardisExchange::Deribit, Some("BTC_USDC"), None)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id.as_str(), "BTC_USDC");
}

#[rstest]
#[tokio::test]
async fn test_instruments_info_api_error() {
    let app = Router::new().route(
        "/instruments/{exchange}",
        get(|| async {
            (
                StatusCode::FORBIDDEN,
                r#"{"code": 403, "message": "Invalid API key"}"#,
            )
                .into_response()
        }),
    );
    let (addr, _handle) = start_mock_server(app).await;
    let client = create_client(addr);

    let result = client
        .instruments_info(TardisExchange::Deribit, None, None)
        .await;

    assert!(result.is_err());

    if let Err(Error::ApiError {
        status,
        code,
        message,
    }) = result
    {
        assert_eq!(status, 403);
        assert_eq!(code, 403);
        assert_eq!(message, "Invalid API key");
    } else {
        panic!("Expected ApiError");
    }
}

#[rstest]
#[tokio::test]
async fn test_instruments_info_malformed_response() {
    let app = Router::new().route(
        "/instruments/{exchange}",
        get(|| async { "not valid json at all" }),
    );
    let (addr, _handle) = start_mock_server(app).await;
    let client = create_client(addr);

    let result = client
        .instruments_info(TardisExchange::Deribit, None, None)
        .await;

    assert!(matches!(result, Err(Error::ResponseParse(_))));
}

#[rstest]
#[tokio::test]
async fn test_bootstrap_instruments_returns_map_and_instruments() {
    let body = format!("[{SPOT_FIXTURE},{PERPETUAL_FIXTURE}]");
    let app = Router::new().route(
        "/instruments/{exchange}",
        get(move || {
            let body = body.clone();
            async move { body }
        }),
    );
    let (addr, _handle) = start_mock_server(app).await;
    let client = create_client(addr);

    let mut exchanges = AHashSet::new();
    exchanges.insert(TardisExchange::Deribit);

    let (map, instruments) = client.bootstrap_instruments(&exchanges).await.unwrap();

    assert!(!map.is_empty());
    assert!(!instruments.is_empty());
}
