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

//! L1 API credential creation and derivation for the Polymarket CLOB.

use std::collections::HashMap;

use nautilus_core::{string::secret::SecretString, time::get_atomic_clock_realtime};
use nautilus_network::http::{HttpClient, Method};
use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
    common::{credential::EvmPrivateKey, urls::clob_http_url},
    http::error::{Error, Result, decode_response},
    signing::eip712::sign_clob_auth,
};

/// API credentials returned by the Polymarket CLOB auth endpoints.
#[derive(Debug, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct ApiCredentials {
    pub api_key: SecretString,
    pub secret: SecretString,
    pub passphrase: SecretString,
}

/// Creates new API credentials via `POST /auth/api-key` using L1 authentication.
///
/// Fails if credentials already exist for this `(address, nonce)` pair.
/// Use [`derive_api_key`] to retrieve existing credentials, or
/// [`create_or_derive_api_key`] for idempotent behavior.
pub async fn create_api_key(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<ApiCredentials> {
    let (client, headers, base) = prepare_l1_request(private_key, nonce, base_url)?;
    let url = format!("{base}/auth/api-key");
    let response = client
        .request(Method::POST, url, None, Some(headers), None, None, None)
        .await
        .map_err(Error::from_http_client)?;

    decode_response(&response)
}

/// Derives existing API credentials via `GET /auth/derive-api-key` using L1 authentication.
///
/// Fails if no credentials exist for this `(address, nonce)` pair.
/// Use [`create_api_key`] to create new credentials, or
/// [`create_or_derive_api_key`] for idempotent behavior.
pub async fn derive_api_key(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<ApiCredentials> {
    let (client, headers, base) = prepare_l1_request(private_key, nonce, base_url)?;
    let url = format!("{base}/auth/derive-api-key");
    let response = client
        .request(Method::GET, url, None, Some(headers), None, None, None)
        .await
        .map_err(Error::from_http_client)?;

    decode_response(&response)
}

/// Creates or derives API credentials using L1 (EIP-712) authentication.
///
/// First attempts `POST /auth/api-key` (create). On HTTP-level errors
/// (e.g. nonce already used), falls back to `GET /auth/derive-api-key`
/// (derive). Transport and network errors are propagated immediately
/// without attempting the fallback.
pub async fn create_or_derive_api_key(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<ApiCredentials> {
    match create_api_key(private_key, nonce, base_url).await {
        Ok(creds) => Ok(creds),
        Err(e) if e.is_http_status_error() => derive_api_key(private_key, nonce, base_url).await,
        Err(e) => Err(e),
    }
}

fn prepare_l1_request(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<(HttpClient, HashMap<String, String>, String)> {
    let base = base_url
        .unwrap_or_else(|| clob_http_url())
        .trim_end_matches('/')
        .to_string();
    let timestamp =
        (get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000_000).to_string();
    let (address, signature) = sign_clob_auth(private_key, &timestamp, nonce)?;
    let headers = l1_headers(&address, &signature, &timestamp, nonce);
    let client = HttpClient::builder()
        .build()
        .map_err(Error::from_http_client)?;
    Ok((client, headers, base))
}

fn l1_headers(
    address: &str,
    signature: &str,
    timestamp: &str,
    nonce: u64,
) -> HashMap<String, String> {
    HashMap::from([
        ("POLY_ADDRESS".to_string(), address.to_string()),
        ("POLY_SIGNATURE".to_string(), signature.to_string()),
        ("POLY_TIMESTAMP".to_string(), timestamp.to_string()),
        ("POLY_NONCE".to_string(), nonce.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Router,
        extract::State,
        http::{HeaderMap, Method as AxumMethod, StatusCode, Uri},
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use rstest::rstest;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        sync::Mutex,
        task::JoinHandle,
    };

    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
    const API_CREDENTIALS_RESPONSE: &str =
        include_str!("../../test_data/http_api_credentials.json");
    const ERROR_RESPONSE: &str = include_str!("../../test_data/http_order_response_error_500.json");
    const WRONG_SCHEMA_RESPONSE: &str = include_str!("../../test_data/http_empty_page.json");

    type RequestLog = Arc<Mutex<Vec<RecordedRequest>>>;

    #[derive(Clone, Copy)]
    struct TestResponse {
        status: StatusCode,
        body: &'static str,
    }

    #[derive(Clone)]
    struct TestServerState {
        create_response: TestResponse,
        derive_response: TestResponse,
        requests: RequestLog,
    }

    #[derive(Debug)]
    struct RecordedRequest {
        method: AxumMethod,
        path: String,
        headers: HeaderMap,
    }

    #[rstest]
    fn test_api_credentials_zeroize_and_redact_debug() {
        fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

        let mut credentials = ApiCredentials {
            api_key: SecretString::from("api-key-sentinel"),
            secret: SecretString::from("secret-sentinel"),
            passphrase: SecretString::from("passphrase-sentinel"),
        };

        let debug = format!("{credentials:?}");
        credentials.zeroize();

        assert_zeroize_on_drop::<ApiCredentials>();
        assert!(!debug.contains("api-key-sentinel"));
        assert!(!debug.contains("secret-sentinel"));
        assert!(!debug.contains("passphrase-sentinel"));
        assert_eq!(credentials.api_key.expose_secret(), "");
        assert_eq!(credentials.secret.expose_secret(), "");
        assert_eq!(credentials.passphrase.expose_secret(), "");
    }

    #[rstest]
    #[tokio::test]
    async fn test_create_or_derive_api_key_returns_created_credentials_without_fallback() {
        let (base_url, requests, server) = start_test_server(
            TestResponse {
                status: StatusCode::OK,
                body: API_CREDENTIALS_RESPONSE,
            },
            TestResponse {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: ERROR_RESPONSE,
            },
        )
        .await;
        let private_key = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();
        let timestamp_before = unix_seconds();

        let credentials = create_or_derive_api_key(&private_key, 7, Some(&base_url))
            .await
            .unwrap();
        let timestamp_after = unix_seconds();

        let requests = requests.lock().await;
        assert_credentials(&credentials);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, AxumMethod::POST);
        assert_eq!(requests[0].path, "/auth/api-key");
        assert_l1_headers(
            &requests[0].headers,
            &private_key,
            7,
            timestamp_before,
            timestamp_after,
        );
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_derive_api_key_sends_l1_get_and_decodes_credentials() {
        let (base_url, requests, server) = start_test_server(
            TestResponse {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: ERROR_RESPONSE,
            },
            TestResponse {
                status: StatusCode::OK,
                body: API_CREDENTIALS_RESPONSE,
            },
        )
        .await;
        let private_key = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();
        let timestamp_before = unix_seconds();

        let credentials = derive_api_key(&private_key, 11, Some(&base_url))
            .await
            .unwrap();
        let timestamp_after = unix_seconds();

        let requests = requests.lock().await;
        assert_credentials(&credentials);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, AxumMethod::GET);
        assert_eq!(requests[0].path, "/auth/derive-api-key");
        assert_l1_headers(
            &requests[0].headers,
            &private_key,
            11,
            timestamp_before,
            timestamp_after,
        );
        server.abort();
    }

    #[rstest]
    #[case(StatusCode::CONFLICT)]
    #[case(StatusCode::TOO_MANY_REQUESTS)]
    #[tokio::test]
    async fn test_create_or_derive_api_key_falls_back_after_http_error(
        #[case] create_status: StatusCode,
    ) {
        let (base_url, requests, server) = start_test_server(
            TestResponse {
                status: create_status,
                body: ERROR_RESPONSE,
            },
            TestResponse {
                status: StatusCode::OK,
                body: API_CREDENTIALS_RESPONSE,
            },
        )
        .await;
        let private_key = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();
        let timestamp_before = unix_seconds();

        let credentials = create_or_derive_api_key(&private_key, 13, Some(&base_url))
            .await
            .unwrap();
        let timestamp_after = unix_seconds();

        let requests = requests.lock().await;
        assert_credentials(&credentials);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, AxumMethod::POST);
        assert_eq!(requests[0].path, "/auth/api-key");
        assert_eq!(requests[1].method, AxumMethod::GET);
        assert_eq!(requests[1].path, "/auth/derive-api-key");
        assert_l1_headers(
            &requests[0].headers,
            &private_key,
            13,
            timestamp_before,
            timestamp_after,
        );
        assert_l1_headers(
            &requests[1].headers,
            &private_key,
            13,
            timestamp_before,
            timestamp_after,
        );
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_create_or_derive_api_key_does_not_fall_back_after_decode_error() {
        let (base_url, requests, server) = start_test_server(
            TestResponse {
                status: StatusCode::OK,
                body: WRONG_SCHEMA_RESPONSE,
            },
            TestResponse {
                status: StatusCode::OK,
                body: API_CREDENTIALS_RESPONSE,
            },
        )
        .await;
        let private_key = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();
        let timestamp_before = unix_seconds();

        let error = create_or_derive_api_key(&private_key, 17, Some(&base_url))
            .await
            .unwrap_err();
        let timestamp_after = unix_seconds();

        let requests = requests.lock().await;
        assert!(matches!(error, Error::Serde(_)));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, AxumMethod::POST);
        assert_eq!(requests[0].path, "/auth/api-key");
        assert_l1_headers(
            &requests[0].headers,
            &private_key,
            17,
            timestamp_before,
            timestamp_after,
        );
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_create_or_derive_api_key_does_not_fall_back_after_transport_error() {
        let (base_url, requests, server) = start_connection_drop_server().await;
        let private_key = EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap();

        let error = create_or_derive_api_key(&private_key, 19, Some(&base_url))
            .await
            .unwrap_err();

        let requests = requests.lock().await;
        assert!(matches!(error, Error::Transport(_)));
        assert_eq!(requests.as_slice(), ["POST /auth/api-key HTTP/1.1"]);
        server.abort();
    }

    async fn start_test_server(
        create_response: TestResponse,
        derive_response: TestResponse,
    ) -> (String, RequestLog, JoinHandle<()>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = TestServerState {
            create_response,
            derive_response,
            requests: Arc::clone(&requests),
        };
        let app = Router::new()
            .route("/auth/api-key", post(handle_create))
            .route("/auth/derive-api-key", get(handle_derive))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{address}/"), requests, server)
    }

    async fn start_connection_drop_server() -> (String, Arc<Mutex<Vec<String>>>, JoinHandle<()>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut create_stream, _) = listener.accept().await.unwrap();
            let create_request = read_request_line(&mut create_stream).await;
            server_requests.lock().await.push(create_request);
            drop(create_stream);

            let (mut derive_stream, _) = listener.accept().await.unwrap();
            let derive_request = read_request_line(&mut derive_stream).await;
            server_requests.lock().await.push(derive_request);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                API_CREDENTIALS_RESPONSE.len(),
                API_CREDENTIALS_RESPONSE,
            );
            derive_stream.write_all(response.as_bytes()).await.unwrap();
        });

        (format!("http://{address}/"), requests, server)
    }

    async fn read_request_line(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();

        loop {
            let mut chunk = [0; 1024];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&chunk[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        String::from_utf8(request)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string()
    }

    async fn handle_create(
        State(state): State<TestServerState>,
        method: AxumMethod,
        uri: Uri,
        headers: HeaderMap,
    ) -> Response {
        let response = state.create_response;
        record_and_respond(state, method, uri, headers, response).await
    }

    async fn handle_derive(
        State(state): State<TestServerState>,
        method: AxumMethod,
        uri: Uri,
        headers: HeaderMap,
    ) -> Response {
        let response = state.derive_response;
        record_and_respond(state, method, uri, headers, response).await
    }

    async fn record_and_respond(
        state: TestServerState,
        method: AxumMethod,
        uri: Uri,
        headers: HeaderMap,
        response: TestResponse,
    ) -> Response {
        state.requests.lock().await.push(RecordedRequest {
            method,
            path: uri.path().to_string(),
            headers,
        });
        (
            response.status,
            [("content-type", "application/json")],
            response.body,
        )
            .into_response()
    }

    fn assert_credentials(credentials: &ApiCredentials) {
        assert_eq!(credentials.api_key.expose_secret(), "test-api-key");
        assert_eq!(credentials.secret.expose_secret(), "test-secret");
        assert_eq!(credentials.passphrase.expose_secret(), "test-passphrase");
    }

    fn assert_l1_headers(
        headers: &HeaderMap,
        private_key: &EvmPrivateKey,
        nonce: u64,
        timestamp_before: u64,
        timestamp_after: u64,
    ) {
        let address = headers.get("poly_address").unwrap().to_str().unwrap();
        let signature = headers.get("poly_signature").unwrap().to_str().unwrap();
        let timestamp = headers
            .get("poly_timestamp")
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let header_nonce = headers.get("poly_nonce").unwrap().to_str().unwrap();
        let (_, expected_signature) =
            sign_clob_auth(private_key, &timestamp.to_string(), nonce).unwrap();

        assert_eq!(address, TEST_ADDRESS);
        assert_eq!(signature, expected_signature);
        assert!(timestamp >= timestamp_before);
        assert!(timestamp <= timestamp_after);
        assert_eq!(header_nonce, nonce.to_string());
    }

    fn unix_seconds() -> u64 {
        get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000_000
    }
}
