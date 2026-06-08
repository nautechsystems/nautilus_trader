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

//! Integration tests for the Derive instrument provider using an axum mock server.

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::post,
};
use nautilus_common::{providers::InstrumentProvider, testing::wait_until_async};
use nautilus_derive::{http::DeriveHttpClient, providers::DeriveInstrumentProvider};
use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct CapturedRequest {
    path: String,
    body: Value,
}

#[derive(Clone, Default)]
struct TestServerState {
    captured: Arc<tokio::sync::Mutex<Vec<CapturedRequest>>>,
    response_body: Arc<tokio::sync::Mutex<Value>>,
    responses_by_currency: Arc<tokio::sync::Mutex<HashMap<String, Value>>>,
    responses_by_type: Arc<tokio::sync::Mutex<HashMap<String, Value>>>,
}

impl TestServerState {
    fn with_instruments_response() -> Self {
        let state = Self::default();
        *state.response_body.try_lock().unwrap() =
            load_json("common/http_get_instruments_eth_all.json");
        state
    }

    fn with_currency_responses(responses: Vec<(&str, Value)>) -> Self {
        let state = Self::default();
        let mut response_map = state.responses_by_currency.try_lock().unwrap();
        for (currency, response) in responses {
            response_map.insert(currency.to_string(), response);
        }
        drop(response_map);
        state
    }

    fn with_type_overrides(self, overrides: Vec<(&str, Value)>) -> Self {
        let mut map = self.responses_by_type.try_lock().unwrap();
        for (instrument_type, response) in overrides {
            map.insert(instrument_type.to_string(), response);
        }
        drop(map);
        self
    }

    async fn captured_requests(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }
}

async fn handle_get_instruments(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/public/get_instruments", state, headers, body).await
}

async fn handle(
    path: &str,
    state: TestServerState,
    _headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let parsed_body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

    state.captured.lock().await.push(CapturedRequest {
        path: path.to_string(),
        body: parsed_body.clone(),
    });

    // Per-type overrides take precedence so a single test can mix a successful
    // response for one instrument_type with an error response for another
    // (e.g. perp succeeds, erc20 returns the venue's 12001 not-found error).
    let by_type = if let Some(itype) = parsed_body.get("instrument_type").and_then(Value::as_str) {
        state.responses_by_type.lock().await.get(itype).cloned()
    } else {
        None
    };

    let body = if let Some(by_type) = by_type {
        by_type
    } else if let Some(currency) = parsed_body.get("currency").and_then(Value::as_str) {
        let response = state
            .responses_by_currency
            .lock()
            .await
            .get(currency)
            .cloned();

        match response {
            Some(body) => body,
            None => state.response_body.lock().await.clone(),
        }
    } else {
        state.response_body.lock().await.clone()
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = Router::new()
        .route("/public/get_instruments", post(handle_get_instruments))
        .route("/health", axum::routing::get(handle_health))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let health_url = format!("http://{addr}/health");
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

    addr
}

fn base_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn instrument_request(currency: &str, instrument_type: &str, expired: bool) -> Value {
    json!({
        "currency": currency,
        "instrument_type": instrument_type,
        "expired": expired,
    })
}

fn eth_instrument_requests(expired: bool) -> Vec<Value> {
    let mut requests = vec![
        instrument_request("ETH", "perp", expired),
        instrument_request("ETH", "option", expired),
        instrument_request("ETH", "erc20", expired),
    ];
    sort_by_instrument_type(&mut requests);
    requests
}

// Sort by the `instrument_type` field so assertions are order-independent.
// `tokio::try_join!` schedules the underlying fetches in parallel and the mock
// captures them in arrival order, which is not deterministic.
fn request_bodies(requests: Vec<CapturedRequest>) -> Vec<Value> {
    let mut bodies: Vec<Value> = requests.into_iter().map(|request| request.body).collect();
    sort_by_instrument_type(&mut bodies);
    bodies
}

fn sort_by_instrument_type(bodies: &mut [Value]) {
    bodies.sort_by(|a, b| {
        let key = |v: &Value| {
            v.get("instrument_type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        key(a).cmp(&key(b))
    });
}

#[rstest]
#[tokio::test]
async fn test_load_all_includes_spot_rows() {
    // Drive the erc20 arm of the try_join with a real spot envelope so the
    // provider must parse a CurrencyPair and surface it through the store.
    // Without this case, dropping `definitions.extend(erc20s)` would not fail
    // any test (the mixed perp+option fixture has no spot rows).
    let state = TestServerState::with_instruments_response().with_type_overrides(vec![(
        "erc20",
        load_json("spot/http_get_instruments_eth.json"),
    )]);
    let addr = start_mock_server(state).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, vec!["ETH".to_string()]);

    provider.load_all(None).await.unwrap();

    let spot_id = InstrumentId::from("ETH-USDC.DERIVE");
    let spot = provider.store().find(&spot_id).expect("spot loaded");
    assert!(matches!(spot, InstrumentAny::CurrencyPair(_)));
    assert!(
        provider
            .store()
            .contains(&InstrumentId::from("ETH-PERP.DERIVE"))
    );
    assert!(
        provider
            .store()
            .contains(&InstrumentId::from("ETH-20260627-3500-C.DERIVE"))
    );
    assert_eq!(provider.store().count(), 3);
}

#[rstest]
#[tokio::test]
async fn test_load_all_passes_through_non_not_found_erc20_errors() {
    // Only venue code 12001 (Instrument not found) is tolerated. Any other
    // JSON-RPC error on the erc20 arm must fail the whole instrument fetch
    // so that real venue failures are not silently swallowed.
    let state = TestServerState::with_instruments_response().with_type_overrides(vec![(
        "erc20",
        json!({
            "id": 1,
            "error": {"code": 5000, "message": "Internal error"},
        }),
    )]);
    let addr = start_mock_server(state).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, vec!["ETH".to_string()]);

    let err = provider
        .load_all(None)
        .await
        .expect_err("non-12001 RPC error must fail load_all");

    assert!(
        err.to_string().contains("5000"),
        "expected error to mention venue code, was: {err}"
    );
}

#[rstest]
#[tokio::test]
async fn test_load_all_tolerates_erc20_instrument_not_found() {
    // Venue returns JSON-RPC 12001 for currencies without a spot listing
    // (e.g. BTC has perp+option but no spot). The provider must keep the
    // perp+option rows instead of failing the whole fetch.
    let state = TestServerState::with_instruments_response().with_type_overrides(vec![(
        "erc20",
        json!({
            "id": 1,
            "error": {"code": 12001, "message": "Instrument not found"},
        }),
    )]);
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, vec!["ETH".to_string()]);

    provider.load_all(None).await.unwrap();

    let perp_id = InstrumentId::from("ETH-PERP.DERIVE");
    let option_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");
    assert!(provider.store().contains(&perp_id));
    assert!(provider.store().contains(&option_id));
    assert_eq!(provider.store().count(), 2);
}

#[rstest]
#[tokio::test]
async fn test_load_all_fetches_parses_and_caches_instruments() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, vec!["ETH".to_string()]);

    provider.load_all(None).await.unwrap();

    let requests = state.captured_requests().await;
    let perp_id = InstrumentId::from("ETH-PERP.DERIVE");
    let option_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");
    let perp = provider.store().find(&perp_id).expect("perp loaded");
    let option = provider.store().find(&option_id).expect("option loaded");

    assert!(
        requests
            .iter()
            .all(|request| request.path == "/public/get_instruments")
    );
    assert_eq!(request_bodies(requests), eth_instrument_requests(false));
    assert!(provider.store().is_initialized());
    assert_eq!(provider.store().count(), 2);
    assert!(matches!(perp, InstrumentAny::CryptoPerpetual(_)));
    assert!(matches!(option, InstrumentAny::CryptoOption(_)));
}

#[rstest]
#[tokio::test]
async fn test_load_ids_fetches_currency_from_instrument_id() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, Vec::new());
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    provider.load_ids(&[instrument_id], None).await.unwrap();

    let requests = state.captured_requests().await;
    assert_eq!(request_bodies(requests), eth_instrument_requests(false));
    assert!(provider.store().contains(&instrument_id));
}

#[rstest]
#[tokio::test]
async fn test_load_fetches_single_instrument() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, Vec::new());
    let instrument_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");

    provider.load(&instrument_id, None).await.unwrap();

    let requests = state.captured_requests().await;
    let instrument = provider
        .store()
        .find(&instrument_id)
        .expect("instrument loaded");

    assert_eq!(request_bodies(requests), eth_instrument_requests(false));
    assert!(matches!(instrument, InstrumentAny::CryptoOption(_)));
}

#[rstest]
#[tokio::test]
async fn test_load_ids_preserves_existing_instruments_when_fallback_misses() {
    let state = TestServerState::with_currency_responses(vec![
        ("ETH", load_json("common/http_get_instruments_eth_all.json")),
        (
            "BTC",
            load_json("common/http_get_instruments_btc_empty.json"),
        ),
    ]);
    let addr = start_mock_server(state).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, vec!["BTC".to_string()]);
    let existing_id = InstrumentId::from("ETH-PERP.DERIVE");
    let missing_id = InstrumentId::from("BTC-PERP.DERIVE");

    provider.load_ids(&[existing_id], None).await.unwrap();
    let err = provider
        .load_ids(&[missing_id], None)
        .await
        .expect_err("missing instrument must error");

    assert!(err.to_string().contains("Derive instruments not found"));
    assert!(provider.store().contains(&existing_id));
    assert!(!provider.store().contains(&missing_id));
}

#[rstest]
#[tokio::test]
async fn test_load_ids_does_not_request_empty_currency() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, Vec::new());
    let instrument_id = InstrumentId::from("-FOO.DERIVE");

    let err = provider
        .load_ids(&[instrument_id], None)
        .await
        .expect_err("empty currency must not fetch");

    assert!(
        err.to_string()
            .contains("DeriveInstrumentProvider requires at least one currency")
    );
    assert!(state.captured_requests().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_load_all_deduplicates_currencies_filter() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, Vec::new());
    let filters = HashMap::from([("currencies".to_string(), "ETH,ETH".to_string())]);

    provider.load_all(Some(&filters)).await.unwrap();

    let captured = state.captured_requests().await;
    assert_eq!(request_bodies(captured), eth_instrument_requests(false));
}

#[rstest]
#[tokio::test]
async fn test_load_all_uses_include_expired_default() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider =
        DeriveInstrumentProvider::with_expired(client, vec!["ETH".to_string()], true);

    provider.load_all(None).await.unwrap();

    let captured = state.captured_requests().await;
    assert_eq!(request_bodies(captured), eth_instrument_requests(true));
}

#[rstest]
#[tokio::test]
async fn test_load_all_expired_filter_overrides_default() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider =
        DeriveInstrumentProvider::with_expired(client, vec!["ETH".to_string()], false);
    let filters = HashMap::from([("expired".to_string(), "true".to_string())]);

    provider.load_all(Some(&filters)).await.unwrap();

    let captured = state.captured_requests().await;
    assert_eq!(request_bodies(captured), eth_instrument_requests(true));
}

#[rstest]
#[tokio::test]
async fn test_load_all_rejects_invalid_expired_filter_before_request() {
    let state = TestServerState::with_instruments_response();
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let mut provider = DeriveInstrumentProvider::new(client, vec!["ETH".to_string()]);
    let filters = HashMap::from([("expired".to_string(), "not-bool".to_string())]);

    let err = provider
        .load_all(Some(&filters))
        .await
        .expect_err("invalid expired filter must fail");

    assert!(err.to_string().contains("invalid Derive `expired` filter"));
    assert!(state.captured_requests().await.is_empty());
}
