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

//! Session lifecycle tests against a local HTTP server.

use std::sync::Arc;

use aws_lc_rs::hmac;
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE};
use nautilus_polymarket::session::{PolymarketSessionKeyClient, PolymarketSessionKeyClientConfig};
use rstest::rstest;
use serde_json::{Value, json};

const PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const WALLET: &str = "0x1111111111111111111111111111111111111111";
const SESSION: &str = "0x2222222222222222222222222222222222222222";

#[derive(Default)]
struct Recorded {
    requests: Vec<(HeaderMap, Vec<u8>)>,
    key: Option<Value>,
    failed_transaction: bool,
    reject: bool,
    retry: bool,
    revoked: bool,
    unavailable: bool,
    malformed_revocation: Option<Value>,
    expired_registry: bool,
    registry_reads: usize,
    transient_reads: usize,
    transaction_reads: usize,
}
type Shared = Arc<tokio::sync::Mutex<Recorded>>;

#[tokio::test]
async fn authorize_list_revoke_session() {
    let state = Shared::default();
    let (client, server) = start(state.clone()).await;
    let key = client.authorize_session_key(SESSION).await.unwrap();
    assert_eq!(client.list_session_keys().await.unwrap(), vec![key.clone()]);
    client.revoke_session_key(SESSION).await.unwrap();
    let remaining = client.list_session_keys().await.unwrap();
    let state = state.lock().await;
    let authorization: Value = serde_json::from_slice(&state.requests[0].1).unwrap();
    let revocation: Value = serde_json::from_slice(&state.requests[1].1).unwrap();
    assert_eq!(key.address, SESSION);
    assert_eq!(key.scopes, ["CLOB"]);
    assert_eq!(key.valid_until.to_string(), authorization["validUntil"]);
    assert_eq!(authorization["walletAddress"], WALLET);
    assert_eq!(authorization["nonce"], "7");
    assert_eq!(revocation["sessionSignerAddress"], SESSION);
    assert_eq!(revocation["walletAddress"], WALLET);
    assert_eq!(revocation.get("validUntil"), None);
    assert_eq!(revocation.get("scopes"), None);
    assert_eq!(remaining, vec![]);
    assert!(state.revoked);
    server.abort();
}

#[tokio::test]
async fn list_omits_expired_session_keys() {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        expired_registry: true,
        ..Default::default()
    }));

    let (client, server) = start(state).await;
    let keys = client.list_session_keys().await.unwrap();
    assert_eq!(keys, vec![]);
    server.abort();
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn authorization_failures(#[case] failed_transaction: bool) {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        failed_transaction,
        reject: !failed_transaction,
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    let error = client.authorize_session_key(SESSION).await.unwrap_err();

    if failed_transaction {
        assert_eq!(
            error.to_string(),
            "exchange error: Session transaction failed: tx-1"
        );
    } else {
        assert_eq!(error.to_string(), "HTTP error 403: denied");
    }

    assert_eq!(state.lock().await.requests.len(), 1);
    server.abort();
}

#[tokio::test]
async fn retry_preserves_signed_payload_and_idempotency_key() {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        retry: true,
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    client.authorize_session_key(SESSION).await.unwrap();
    let state = state.lock().await;
    assert_eq!(state.requests.len(), 2);
    assert_eq!(state.requests[0].1, state.requests[1].1);
    assert_eq!(
        state.requests[0].0["idempotency-key"],
        state.requests[1].0["idempotency-key"]
    );
    server.abort();
}

async fn start(state: Shared) -> (PolymarketSessionKeyClient, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route(
            "/v1/account/transactions/params",
            get(|| async { Json(json!({"nonce":"7"})) }),
        )
        .route("/v1/account/transactions/tx-1", get(transaction))
        .route("/v1/account/transactions/tx-2", get(revocation_transaction))
        .route("/v1/session-signers/authorizations", post(authorize))
        .route("/v1/session-signers/revocations", post(revoke))
        .route("/v1/user/session-signers", get(list))
        .with_state(state);

    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let client = PolymarketSessionKeyClient::new(PolymarketSessionKeyClientConfig {
        private_key: PRIVATE_KEY.into(),
        api_key: "owner-api".into(),
        api_secret: "c2VjcmV0".into(),
        passphrase: "owner-pass".into(),
        builder_api_key: "builder-api".into(),
        builder_api_secret: "YnVpbGRlcg==".into(),
        builder_passphrase: "builder-pass".into(),
        funder: WALLET.into(),
        base_url_http: Some(url.clone()),
        base_url_relayer: Some(url),
        proxy_url: None,
    })
    .unwrap();

    (client, server)
}

async fn authorize(
    State(state): State<Shared>,
    headers: HeaderMap,
    bytes: Bytes,
) -> (StatusCode, Json<Value>) {
    assert_builder_auth(&headers, "/v1/session-signers/authorizations", &bytes);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let mut state = state.lock().await;
    state.requests.push((headers, bytes.to_vec()));
    if state.reject {
        return (StatusCode::FORBIDDEN, Json(json!({"error":"denied"})));
    }

    if state.unavailable || (state.retry && state.requests.len() == 1) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"retry"})),
        );
    }

    state.key = Some(
        json!({"address": SESSION, "scopes":["CLOB"], "valid_until": body["validUntil"].as_str().unwrap().parse::<u64>().unwrap()}),
    );
    (
        StatusCode::OK,
        Json(json!({"status":"SUBMITTED","transactionId":"tx-1"})),
    )
}

async fn revoke(State(state): State<Shared>, headers: HeaderMap, bytes: Bytes) -> Json<Value> {
    assert_builder_auth(&headers, "/v1/session-signers/revocations", &bytes);
    let mut state = state.lock().await;
    state.requests.push((headers, bytes.to_vec()));
    if let Some(response) = &state.malformed_revocation {
        return Json(response.clone());
    }

    state.revoked = true;
    state.key = None;
    Json(json!({"status":"FENCED","fenced":true,"transactionId":"tx-2"}))
}

async fn list(State(state): State<Shared>, headers: HeaderMap) -> (StatusCode, Json<Value>) {
    assert_eq!(headers["poly_api_key"], "owner-api");
    assert_eq!(
        headers["poly_address"],
        "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
    );
    let mut state = state.lock().await;
    state.registry_reads += 1;
    if state.expired_registry {
        return (
            StatusCode::OK,
            Json(
                json!({"wallet":WALLET,"signers":[{"address":SESSION,"scopes":["CLOB"],"valid_until":0}]}),
            ),
        );
    }

    if state.transient_reads > 0 {
        state.transient_reads -= 1;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"retry"})),
        );
    }

    (
        StatusCode::OK,
        Json(json!({"wallet":WALLET,"signers":state.key.iter().cloned().collect::<Vec<_>>()})),
    )
}

async fn transaction(State(state): State<Shared>) -> Json<Value> {
    let state = if state.lock().await.failed_transaction {
        "STATE_FAILED"
    } else {
        "STATE_CONFIRMED"
    };

    Json(json!({"transaction_id":"tx-1","state":state}))
}

async fn revocation_transaction(State(state): State<Shared>) -> Json<Value> {
    state.lock().await.transaction_reads += 1;
    Json(json!({"transaction_id":"tx-2","state":"STATE_CONFIRMED"}))
}

#[tokio::test]
async fn unresolved_submission_resumes_identical_request() {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        unavailable: true,
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    let error = client.authorize_session_key(SESSION).await.unwrap_err();
    let blocked = client.revoke_session_key(SESSION).await.unwrap_err();
    state.lock().await.unavailable = false;
    let key = client.authorize_session_key(SESSION).await.unwrap();
    let state = state.lock().await;
    let idempotency_key = state.requests[0].0["idempotency-key"].to_str().unwrap();
    assert!(error.to_string().contains(&format!(
        "idempotency_key={idempotency_key}, transaction_id=unknown"
    )));
    assert!(
        blocked
            .to_string()
            .contains("Resume the previous session operation first")
    );
    assert_eq!(key.address, SESSION);
    assert_eq!(state.requests.len(), 4);

    for request in &state.requests[1..] {
        assert_eq!(request.1, state.requests[0].1);
        assert_eq!(
            request.0["idempotency-key"],
            state.requests[0].0["idempotency-key"]
        );
    }

    server.abort();
}

#[rstest]
#[case(json!({"status":"PENDING","fenced":true}))]
#[case(json!({"status":"PENDING","transactionId":"tx-2"}))]
#[case(json!({"status":"PENDING","transactionId":"tx-2","fenced":"true"}))]
#[tokio::test]
async fn malformed_revocation_cannot_report_success_and_can_resume(#[case] response: Value) {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        malformed_revocation: Some(response),
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    let error = client.revoke_session_key(SESSION).await.unwrap_err();
    state.lock().await.malformed_revocation = None;
    client.revoke_session_key(SESSION).await.unwrap();
    let state = state.lock().await;
    assert!(error.to_string().contains("session operation unresolved"));
    assert_eq!(state.requests.len(), 2);
    assert_eq!(state.requests[0].1, state.requests[1].1);
    assert_eq!(
        state.requests[0].0["idempotency-key"],
        state.requests[1].0["idempotency-key"]
    );
    assert_eq!(state.transaction_reads, 1);
    server.abort();
}

#[tokio::test]
async fn confirmation_retries_transient_registry_failure() {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        transient_reads: 1,
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    let key = client.authorize_session_key(SESSION).await.unwrap();
    let state = state.lock().await;
    assert_eq!(key.address, SESSION);
    assert_eq!(state.transient_reads, 0);
    assert_eq!(state.requests.len(), 1);
    server.abort();
}

#[tokio::test]
async fn cancellation_retains_the_submission_for_retry() {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        unavailable: true,
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    let client = Arc::new(client);
    let task_client = client.clone();
    let task = tokio::spawn(async move { task_client.authorize_session_key(SESSION).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while state.lock().await.requests.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    state.lock().await.unavailable = false;
    client.authorize_session_key(SESSION).await.unwrap();
    let state = state.lock().await;
    assert_eq!(state.requests.len(), 2);
    assert_eq!(state.requests[0].1, state.requests[1].1);
    assert_eq!(
        state.requests[0].0["idempotency-key"],
        state.requests[1].0["idempotency-key"]
    );
    server.abort();
}

#[tokio::test]
async fn terminal_failure_allows_another_operation() {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        failed_transaction: true,
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    let error = client.authorize_session_key(SESSION).await.unwrap_err();
    client.revoke_session_key(SESSION).await.unwrap();
    let state = state.lock().await;
    assert_eq!(
        error.to_string(),
        "exchange error: Session transaction failed: tx-1"
    );
    assert_eq!(state.requests.len(), 2);
    assert_ne!(
        state.requests[0].0["idempotency-key"],
        state.requests[1].0["idempotency-key"]
    );
    assert_eq!(state.transaction_reads, 1);
    server.abort();
}

#[tokio::test]
async fn authorization_waits_for_matching_unexpired_registry_entry() {
    let state = Arc::new(tokio::sync::Mutex::new(Recorded {
        expired_registry: true,
        ..Default::default()
    }));

    let (client, server) = start(state.clone()).await;
    let pending = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        client.authorize_session_key(SESSION),
    )
    .await;
    let reads = state.lock().await.registry_reads;
    state.lock().await.expired_registry = false;
    let key = client.authorize_session_key(SESSION).await.unwrap();
    let state = state.lock().await;
    let request: Value = serde_json::from_slice(&state.requests[0].1).unwrap();
    assert!(
        pending.is_err(),
        "expired registry entry must not complete authorization"
    );
    assert_eq!(reads, 1);
    assert_eq!(key.valid_until.to_string(), request["validUntil"]);
    assert_eq!(state.requests.len(), 1);
    server.abort();
}

fn assert_builder_auth(headers: &HeaderMap, path: &str, body: &[u8]) {
    let timestamp = headers["poly_builder_timestamp"].to_str().unwrap();
    let payload = [timestamp.as_bytes(), b"POST", path.as_bytes(), body].concat();
    let key = hmac::Key::new(hmac::HMAC_SHA256, b"builder");
    let expected = URL_SAFE.encode(hmac::sign(&key, &payload));

    assert_eq!(headers["poly_builder_api_key"], "builder-api");
    assert_eq!(headers["poly_builder_passphrase"], "builder-pass");
    assert_eq!(headers["poly_builder_signature"], expected);
    assert!(!headers.contains_key("relayer_api_key"));
}
