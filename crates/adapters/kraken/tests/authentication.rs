// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Integration tests for Kraken WebSocket authentication.

use axum::{Router, body::Body, extract::Request, http::StatusCode, response::Response};
use nautilus_kraken::{
    config::KrakenDataClientConfig, http::client::KrakenHttpClient,
    websocket::client::KrakenWebSocketClient,
};
use rstest::rstest;
use tokio_util::sync::CancellationToken;

/// Mock HTTP server for testing authentication.
async fn mock_get_websockets_token() -> Response {
    let response_body = r#"{
        "error": [],
        "result": {
            "token": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "expires": 900
        }
    }"#;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(response_body))
        .unwrap()
}

async fn mock_handler(req: Request) -> Response {
    match req.uri().path() {
        "/0/private/GetWebSocketsToken" => mock_get_websockets_token().await,
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not found"))
            .unwrap(),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_client_get_websockets_token() {
    // Start mock HTTP server
    let app = Router::new().fallback(mock_handler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create HTTP client with credentials (API secret must be base64-encoded)
    let client = KrakenHttpClient::with_credentials(
        "test_api_key".to_string(),
        "dGVzdF9hcGlfc2VjcmV0X2Jhc2U2NA==".to_string(), // Base64 encoded "test_api_secret_base64"
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    // Get WebSocket token
    let token = client.get_websockets_token().await.unwrap();

    assert_eq!(token.token, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(token.expires, 900);
}

#[rstest]
#[tokio::test]
async fn test_websocket_client_authenticate() {
    // Start mock HTTP server
    let app = Router::new().fallback(mock_handler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create WebSocket client with credentials (API secret must be base64-encoded)
    let config = KrakenDataClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("dGVzdF9hcGlfc2VjcmV0X2Jhc2U2NA==".to_string()), // Base64 encoded
        base_url: Some(base_url),
        ..Default::default()
    };

    let token = CancellationToken::new();
    let client = KrakenWebSocketClient::new(config, token);

    // Authenticate
    let result = client.authenticate().await;
    assert!(result.is_ok(), "Authentication failed: {result:?}");
}

#[rstest]
#[tokio::test]
async fn test_websocket_client_authenticate_without_credentials() {
    let config = KrakenDataClientConfig::default();
    let token = CancellationToken::new();
    let client = KrakenWebSocketClient::new(config, token);

    // Try to authenticate without credentials
    let result = client.authenticate().await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("credentials"),
        "Expected authentication error"
    );
}
