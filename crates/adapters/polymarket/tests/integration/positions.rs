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

//! Integration tests for Deposit Wallet split, merge, and redeem operations.

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{
        Arc, Once,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use log::{Level, LevelFilter, Log, Metadata, Record};
use nautilus_common::testing::wait_until_async;
use nautilus_network::http::HttpClient;
use nautilus_polymarket::{
    common::credential::{EvmPrivateKey, RelayerApiKey},
    http::{
        error::Error,
        relayer::{PolymarketRelayerHttpClient, RelayerWalletSubmit},
    },
    positions::{PolymarketPositionClient, PolymarketPositionOutcome},
    signing::eip712::{
        CTF_COLLATERAL_ADAPTER, DEPOSIT_WALLET_FACTORY, NEG_RISK_CTF_COLLATERAL_ADAPTER,
    },
};
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

const TEST_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const TEST_SIGNER: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
const TEST_DEPOSIT_WALLET: &str = "0x1111111111111111111111111111111111111111";
const CONDITION_ID: &str = "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Clone)]
struct TestServerState {
    wallet: Arc<tokio::sync::Mutex<String>>,
    owner: Arc<tokio::sync::Mutex<String>>,
    market: Arc<tokio::sync::Mutex<Value>>,
    market_status: Arc<tokio::sync::Mutex<StatusCode>>,
    nonce: Arc<tokio::sync::Mutex<Value>>,
    submit_status: Arc<tokio::sync::Mutex<StatusCode>>,
    submit_body: Arc<tokio::sync::Mutex<Value>>,
    submit_delay: Arc<tokio::sync::Mutex<Duration>>,
    submit_count: Arc<AtomicUsize>,
    rpc_code: Arc<tokio::sync::Mutex<String>>,
    rpc_error: Arc<tokio::sync::Mutex<bool>>,
    rpc_identity: Arc<tokio::sync::Mutex<Option<Value>>>,
    legacy_wallet: Arc<tokio::sync::Mutex<Option<String>>>,
    submit_empty_body: Arc<tokio::sync::Mutex<bool>>,
    last_submit: Arc<tokio::sync::Mutex<Option<Value>>>,
    last_headers: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    poll_pages: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
    poll_statuses: Arc<tokio::sync::Mutex<VecDeque<StatusCode>>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            wallet: Arc::new(tokio::sync::Mutex::new(TEST_DEPOSIT_WALLET.into())),
            owner: Arc::new(tokio::sync::Mutex::new(TEST_SIGNER.into())),
            market: Arc::new(tokio::sync::Mutex::new(market_json(false))),
            market_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            nonce: Arc::new(tokio::sync::Mutex::new(
                json!({"address": TEST_SIGNER, "nonce": "7"}),
            )),
            submit_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            submit_body: Arc::new(tokio::sync::Mutex::new(json!({
                "transactionID": "tx-1",
                "state": "STATE_NEW"
            }))),
            submit_delay: Arc::new(tokio::sync::Mutex::new(Duration::ZERO)),
            submit_count: Arc::new(AtomicUsize::new(0)),
            rpc_code: Arc::new(tokio::sync::Mutex::new("0x1234".into())),
            rpc_error: Arc::new(tokio::sync::Mutex::new(false)),
            rpc_identity: Arc::new(tokio::sync::Mutex::new(None)),
            legacy_wallet: Arc::new(tokio::sync::Mutex::new(None)),
            submit_empty_body: Arc::new(tokio::sync::Mutex::new(false)),
            last_submit: Arc::new(tokio::sync::Mutex::new(None)),
            last_headers: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            poll_pages: Arc::new(tokio::sync::Mutex::new(VecDeque::from([json!({
                "transaction_id": "tx-1",
                "transaction_hash": "0xabc",
                "state": "STATE_CONFIRMED",
                "error_msg": null
            })]))),
            poll_statuses: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
        }
    }
}

fn market_json(neg_risk: bool) -> Value {
    json!({
        "condition_id": CONDITION_ID,
        "closed": false,
        "neg_risk": neg_risk,
        "tokens": []
    })
}

async fn handle_health() -> &'static str {
    "ok"
}

async fn handle_market(State(state): State<TestServerState>) -> Response {
    let status = *state.market_status.lock().await;
    let body = state.market.lock().await.clone();
    (status, Json(body)).into_response()
}

async fn handle_nonce(State(state): State<TestServerState>) -> Json<Value> {
    Json(state.nonce.lock().await.clone())
}

async fn handle_submit(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut captured = HashMap::new();

    for (name, value) in &headers {
        if let Ok(value) = value.to_str() {
            captured.insert(name.as_str().to_string(), value.to_string());
        }
    }

    *state.last_headers.lock().await = captured;
    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
        *state.last_submit.lock().await = Some(value);
    }

    state.submit_count.fetch_add(1, Ordering::SeqCst);
    let delay = *state.submit_delay.lock().await;
    if delay > Duration::ZERO {
        tokio::time::sleep(delay).await;
    }

    let status = *state.submit_status.lock().await;
    if *state.submit_empty_body.lock().await {
        return status.into_response();
    }

    let body = state.submit_body.lock().await.clone();
    (status, Json(body)).into_response()
}

async fn handle_transaction(
    State(state): State<TestServerState>,
    Path(_id): Path<String>,
) -> Response {
    let status = state
        .poll_statuses
        .lock()
        .await
        .pop_front()
        .unwrap_or(StatusCode::OK);
    let mut pages = state.poll_pages.lock().await;
    let body = pages
        .pop_front()
        .unwrap_or_else(|| json!({"transaction_id": "tx-1", "state": "STATE_NEW"}));
    (status, Json(body)).into_response()
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    *state.wallet.lock().await = wallet_address(&addr);
    let router = Router::new()
        .route("/", post(handle_rpc))
        .route("/health", get(handle_health))
        .route("/markets/{condition_id}", get(handle_market))
        .route("/v1/account/transactions/params", get(handle_nonce))
        .route("/submit", post(handle_submit))
        .route("/v1/account/transactions/{id}", get(handle_transaction))
        .with_state(state);

    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    wait_until_async(
        || async move {
            HttpClient::builder()
                .build()
                .unwrap()
                .get(format!("http://{addr}/health"), None, None, Some(1), None)
                .await
                .is_ok()
        },
        Duration::from_secs(5),
    )
    .await;

    addr
}

fn position_client(addr: &SocketAddr) -> PolymarketPositionClient {
    let base_url = Some(format!("http://{addr}"));
    PolymarketPositionClient::new(
        &EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap(),
        &wallet_address(addr),
        RelayerApiKey::new("relayer-key".into(), TEST_SIGNER).unwrap(),
        base_url.clone(),
        base_url,
        Some(2),
        None,
    )
    .unwrap()
    .with_rpc_url(format!("http://{addr}/"))
    .with_deadline_secs(60)
    .with_wait_timeout(Duration::from_millis(400))
    .with_poll_interval(Duration::from_millis(10))
}

#[rstest]
#[tokio::test]
async fn test_split_position_confirms_and_targets_standard_adapter() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = position_client(&addr);

    let tx = client.split_position(CONDITION_ID, dec!(1)).await.unwrap();
    assert_eq!(tx.transaction_id(), "tx-1");
    let outcome = tx.wait().await.unwrap();
    assert_eq!(
        outcome,
        PolymarketPositionOutcome::Confirmed {
            transaction_id: "tx-1".into(),
            transaction_hash: Some("0xabc".into()),
        }
    );

    let submit = state.last_submit.lock().await.clone().unwrap();
    assert_eq!(submit["type"], "WALLET");
    assert_eq!(submit["from"], TEST_SIGNER);
    assert_eq!(submit["to"], format!("{DEPOSIT_WALLET_FACTORY:#x}"));
    assert_eq!(submit["metadata"], "Split position");
    assert_eq!(
        submit["depositWalletParams"]["depositWallet"],
        wallet_address(&addr)
    );
    assert_eq!(
        submit["depositWalletParams"]["calls"][0]["target"],
        format!("{CTF_COLLATERAL_ADAPTER:#x}")
    );
    assert_eq!(submit["depositWalletParams"]["calls"][0]["value"], "0");
    let headers = state.last_headers.lock().await.clone();
    assert_eq!(
        headers.get("relayer_api_key").map(String::as_str),
        Some("relayer-key")
    );
    assert_eq!(
        headers.get("relayer_api_key_address").map(String::as_str),
        Some(TEST_SIGNER)
    );
}

#[rstest]
#[tokio::test]
async fn test_merge_positions_targets_neg_risk_adapter() {
    let state = TestServerState::default();
    *state.market.lock().await = market_json(true);
    let addr = start_mock_server(state.clone()).await;
    let client = position_client(&addr);

    client
        .merge_positions(CONDITION_ID, dec!(0.5))
        .await
        .unwrap();

    let submit = state.last_submit.lock().await.clone().unwrap();
    assert_eq!(
        submit["depositWalletParams"]["calls"][0]["target"],
        format!("{NEG_RISK_CTF_COLLATERAL_ADAPTER:#x}")
    );
}

#[rstest]
#[tokio::test]
async fn test_redeem_positions_confirms() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let client = position_client(&addr);

    let outcome = client
        .redeem_positions(CONDITION_ID)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        PolymarketPositionOutcome::Confirmed { .. }
    ));
}

#[rstest]
#[tokio::test]
async fn test_missing_neg_risk_is_invalid_metadata() {
    let state = TestServerState::default();
    *state.market.lock().await = json!({
        "condition_id": CONDITION_ID,
        "closed": false,
        "tokens": []
    });
    let addr = start_mock_server(state).await;
    let client = position_client(&addr);

    let err = client
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing neg_risk"));
}

#[rstest]
#[tokio::test]
async fn test_submit_rejection_is_explicit() {
    let state = TestServerState::default();
    *state.submit_status.lock().await = StatusCode::BAD_REQUEST;
    *state.submit_body.lock().await = json!({"error": "invalid signature"});
    let addr = start_mock_server(state).await;
    let client = position_client(&addr);

    let err = client
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("400"));
    assert!(err.to_string().contains("invalid signature"));
}

#[rstest]
#[tokio::test]
async fn test_submit_empty_body_is_ambiguous() {
    let state = TestServerState::default();
    *state.submit_empty_body.lock().await = true;
    let addr = start_mock_server(state).await;
    let client = position_client(&addr);

    let err = client
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("transaction outcome is unknown"));
}

#[rstest]
#[tokio::test]
async fn test_submit_timeout_is_ambiguous() {
    let state = TestServerState::default();
    *state.submit_delay.lock().await = Duration::from_millis(1500);
    let addr = start_mock_server(state).await;
    let base_url = Some(format!("http://{addr}"));
    let client = PolymarketPositionClient::new(
        &EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap(),
        &wallet_address(&addr),
        RelayerApiKey::new("relayer-key".into(), TEST_SIGNER).unwrap(),
        base_url.clone(),
        base_url,
        Some(1),
        None,
    )
    .unwrap()
    .with_rpc_url(format!("http://{addr}/"));

    let err = client
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "timeout");
}

#[rstest]
#[tokio::test]
async fn test_wait_retries_transient_poll_error() {
    let state = TestServerState::default();
    *state.poll_statuses.lock().await =
        VecDeque::from([StatusCode::INTERNAL_SERVER_ERROR, StatusCode::OK]);
    *state.poll_pages.lock().await = VecDeque::from([
        json!({"error": "temporary"}),
        json!({
            "transaction_id": "tx-1",
            "transaction_hash": "0xabc",
            "state": "STATE_CONFIRMED",
            "error_msg": null
        }),
    ]);
    let addr = start_mock_server(state).await;
    let outcome = position_client(&addr)
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        outcome,
        PolymarketPositionOutcome::Confirmed {
            transaction_id: "tx-1".into(),
            transaction_hash: Some("0xabc".into()),
        }
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_omitted_transaction_id_is_ambiguous() {
    let state = TestServerState::default();
    *state.submit_body.lock().await = json!({"state": "STATE_NEW"});
    let addr = start_mock_server(state).await;
    let client = position_client(&addr);

    let err = client
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("transaction outcome is unknown"));
}

#[rstest]
#[tokio::test]
async fn test_wait_failed_and_invalid_terminal_states() {
    let failed_state = TestServerState::default();
    *failed_state.poll_pages.lock().await = VecDeque::from([json!({
        "transaction_id": "tx-1",
        "state": "STATE_FAILED",
        "error_msg": "reverted"
    })]);
    let addr = start_mock_server(failed_state).await;
    let failed = position_client(&addr)
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        failed,
        PolymarketPositionOutcome::Failed {
            transaction_id: "tx-1".into(),
            transaction_hash: None,
            error_msg: Some("reverted".into()),
        }
    );

    let invalid_state = TestServerState::default();
    *invalid_state.poll_pages.lock().await = VecDeque::from([json!({
        "transaction_id": "tx-1",
        "state": "STATE_INVALID",
        "error_msg": "bad nonce"
    })]);
    let addr = start_mock_server(invalid_state).await;
    let invalid = position_client(&addr)
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        invalid,
        PolymarketPositionOutcome::Invalid {
            transaction_id: "tx-1".into(),
            error_msg: Some("bad nonce".into()),
        }
    );
}

#[rstest]
#[tokio::test]
async fn test_wait_timeout_leaves_outcome_unknown() {
    let state = TestServerState::default();
    *state.poll_pages.lock().await = VecDeque::new();
    let addr = start_mock_server(state).await;
    let client = position_client(&addr).with_wait_timeout(Duration::from_millis(50));

    let err = client
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(err.to_string().contains("wait timed out"));
    assert!(err.to_string().contains("tx-1"));
}

#[rstest]
fn test_client_requires_distinct_deposit_wallet() {
    let err = PolymarketPositionClient::new(
        &EvmPrivateKey::new(TEST_PRIVATE_KEY).unwrap(),
        TEST_SIGNER,
        RelayerApiKey::new("relayer-key".into(), TEST_SIGNER).unwrap(),
        None,
        None,
        Some(2),
        None,
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("funder distinct from the signing address")
    );
}

#[rstest]
#[case(Some("unrelated-tx"))]
#[case(None)]
#[tokio::test]
async fn test_wait_rejects_mismatched_transaction_id(#[case] transaction_id: Option<&str>) {
    let state = TestServerState::default();
    *state.poll_pages.lock().await = VecDeque::from([json!({
        "transaction_id": transaction_id,
        "state": "STATE_CONFIRMED"
    })]);
    let addr = start_mock_server(state).await;
    let tx = position_client(&addr)
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap();
    let err = tx.wait().await.unwrap_err();
    assert!(err.to_string().contains("did not match transaction tx-1"));
}

#[rstest]
#[tokio::test]
async fn test_relayer_rejects_cross_origin_redirect() {
    let sink_state = TestServerState::default();
    let sink = start_mock_server(sink_state.clone()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = Router::new().route(
        "/submit",
        post(move || async move {
            (
                StatusCode::TEMPORARY_REDIRECT,
                [("location", format!("http://{sink}/submit"))],
            )
        }),
    );

    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = PolymarketRelayerHttpClient::new(
        RelayerApiKey::new("dummy-redirect-key".into(), TEST_SIGNER).unwrap(),
        Some(format!("http://{addr}")),
        2,
    )
    .unwrap();

    let call = nautilus_polymarket::signing::eip712::DepositWalletCall {
        target: CTF_COLLATERAL_ADAPTER,
        value: Default::default(),
        data: Default::default(),
    };

    let err = client
        .submit_wallet_batch(RelayerWalletSubmit {
            signer: TEST_SIGNER.parse().unwrap(),
            deposit_wallet: TEST_DEPOSIT_WALLET.parse().unwrap(),
            nonce: Default::default(),
            deadline: Default::default(),
            signature: "dummy-signature",
            metadata: "redirect test",
            calls: &[call],
        })
        .await
        .unwrap_err();

    assert!(err.to_string().contains("307"));
    assert_eq!(*sink_state.last_submit.lock().await, None);
    assert_eq!(*sink_state.last_headers.lock().await, HashMap::new());
}

fn wallet_address(addr: &SocketAddr) -> String {
    format!("0x{:040x}", addr.port())
}

#[rstest]
#[case("split", "cancelled")]
#[case("merge", "omitted_id")]
#[case("redeem", "rejected")]
#[case("split", "timeout")]
#[case("merge", "accepted")]
#[tokio::test]
async fn test_recovery_record_precedes_submit_response(
    #[case] operation: &'static str,
    #[case] response: &str,
) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        log::set_logger(&RECOVERY_LOG_CAPTURE).unwrap();
        log::set_max_level(LevelFilter::Info);
    });

    let state = TestServerState::default();
    *state.submit_delay.lock().await = if matches!(response, "cancelled" | "timeout") {
        Duration::from_secs(3)
    } else {
        Duration::from_millis(200)
    };

    if response == "omitted_id" {
        *state.submit_body.lock().await = json!({"state": "STATE_NEW"});
    } else if response == "rejected" {
        *state.submit_status.lock().await = StatusCode::BAD_REQUEST;
    }

    let addr = start_mock_server(state.clone()).await;

    let task = tokio::spawn(async move {
        let client = position_client(&addr);

        match operation {
            "split" => client.split_position(CONDITION_ID, dec!(1.234567)).await,
            "merge" => client.merge_positions(CONDITION_ID, dec!(2.345678)).await,
            "redeem" => client.redeem_positions(CONDITION_ID).await,
            _ => unreachable!(),
        }
    });

    wait_until_async(
        || async { state.submit_count.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(2),
    )
    .await;
    let prefix = format!(
        "Deposit Wallet submission: wallet={},",
        wallet_address(&addr)
    );
    let records: Vec<_> = RECOVERY_LOG_CAPTURE
        .records
        .lock()
        .iter()
        .filter(|(_, message)| message.starts_with(&prefix))
        .cloned()
        .collect();
    let body = state.last_submit.lock().await.clone().unwrap();

    if response == "cancelled" {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    } else {
        let result = task.await.unwrap();

        match response {
            "timeout" => assert!(matches!(result, Err(Error::Timeout))),
            "rejected" => assert!(matches!(result, Err(Error::Http { status: 400, .. }))),
            "omitted_id" => assert_eq!(
                result.unwrap_err().to_string(),
                "decode error: Relayer submit response omitted transaction_id; transaction outcome is unknown",
            ),
            "accepted" => assert_eq!(result.unwrap().transaction_id(), "tx-1"),
            _ => unreachable!(),
        }
    }

    let params = &body["depositWalletParams"];
    let call = &params["calls"][0];
    let expected = format!(
        "{prefix} nonce=7, deadline={}, operation={}, target={}, value=0, data={}",
        params["deadline"].as_str().unwrap(),
        body["metadata"].as_str().unwrap(),
        call["target"].as_str().unwrap(),
        call["data"].as_str().unwrap(),
    );
    assert_eq!(records, vec![(Level::Info, expected)]);
    assert_eq!(body["nonce"], "7");
    assert_eq!(state.submit_count.load(Ordering::SeqCst), 1);
}

struct RecoveryLogCapture {
    records: parking_lot::Mutex<Vec<(Level, String)>>,
}

impl Log for RecoveryLogCapture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() == Level::Info && metadata.target() == "nautilus_polymarket::positions"
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.records
                .lock()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

static RECOVERY_LOG_CAPTURE: RecoveryLogCapture = RecoveryLogCapture {
    records: parking_lot::Mutex::new(Vec::new()),
};

async fn handle_rpc(State(state): State<TestServerState>, Json(body): Json<Value>) -> Json<Value> {
    if *state.rpc_error.lock().await {
        return Json(
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32000, "message": "unavailable"}}),
        );
    }

    let result = if body["method"] == "eth_getCode" {
        state.rpc_code.lock().await.clone()
    } else {
        let data = body["params"][0]["data"].as_str().unwrap();
        let id_selector = alloy_primitives::keccak256(b"id()");

        if data == format!("0x{}", alloy_primitives::hex::encode(&id_selector[..4]))
            && let Some(response) = state.rpc_identity.lock().await.clone()
        {
            return Json(response);
        }

        let nonce_selector = alloy_primitives::keccak256(b"nonce()");

        if data.starts_with(&format!(
            "0x{}",
            alloy_primitives::hex::encode(&nonce_selector[..4])
        )) {
            let nonce: alloy_primitives::U256 = state.nonce.lock().await["nonce"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            return Json(json!({"jsonrpc": "2.0", "id": 1, "result": format!("0x{nonce:064x}")}));
        }

        let owner_selector = alloy_primitives::keccak256(b"owner()");

        if data.starts_with(&format!(
            "0x{}",
            alloy_primitives::hex::encode(&owner_selector[..4])
        )) {
            return Json(
                json!({"jsonrpc": "2.0", "id": 1, "result": format!("0x000000000000000000000000{}", &state.owner.lock().await[2..])}),
            );
        }

        let legacy_selector = alloy_primitives::keccak256(b"predictLegacyWalletAddress(bytes32)");

        let wallet = if data.starts_with(&format!(
            "0x{}",
            alloy_primitives::hex::encode(&legacy_selector[..4])
        )) {
            state
                .legacy_wallet
                .lock()
                .await
                .clone()
                .unwrap_or(state.wallet.lock().await.clone())
        } else {
            state.wallet.lock().await.clone()
        };

        format!("0x000000000000000000000000{}", &wallet[2..])
    };

    Json(json!({"jsonrpc": "2.0", "id": 1, "result": result}))
}

#[rstest]
#[tokio::test]
async fn test_concurrent_clients_do_not_sign_competing_wallet_nonces() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let first = position_client(&addr);
    let second = position_client(&addr);
    let (a, b) = tokio::join!(
        first.split_position(CONDITION_ID, dec!(1)),
        second.merge_positions(CONDITION_ID, dec!(2)),
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let err = a.err().or_else(|| b.err()).unwrap();
    assert!(err.to_string().contains("nonce has not advanced"));
    assert_eq!(state.submit_count.load(Ordering::SeqCst), 1);
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_submit_blocks_recreated_client() {
    let state = TestServerState::default();
    *state.submit_empty_body.lock().await = true;
    let addr = start_mock_server(state.clone()).await;
    let client = position_client(&addr);
    client
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    drop(client);
    let err = position_client(&addr)
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("Previous Deposit Wallet submit outcome is unknown")
    );
    assert_eq!(state.submit_count.load(Ordering::SeqCst), 1);
}

#[rstest]
#[tokio::test]
async fn test_cancelled_submit_retains_wallet_reservation() {
    let state = TestServerState::default();
    *state.submit_delay.lock().await = Duration::from_secs(10);
    let addr = start_mock_server(state.clone()).await;

    let task = tokio::spawn(async move {
        position_client(&addr)
            .split_position(CONDITION_ID, dec!(1))
            .await
    });

    wait_until_async(
        || async { state.submit_count.load(Ordering::SeqCst) == 1 },
        Duration::from_secs(2),
    )
    .await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let err = position_client(&addr)
        .merge_positions(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("Previous Deposit Wallet submit outcome is unknown")
    );
    assert_eq!(state.submit_count.load(Ordering::SeqCst), 1);
}

#[rstest]
#[tokio::test]
async fn test_terminal_submission_allows_advanced_nonce() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = position_client(&addr);
    client.split_position(CONDITION_ID, dec!(1)).await.unwrap();
    *state.nonce.lock().await = json!({"nonce":"8"});
    client.merge_positions(CONDITION_ID, dec!(1)).await.unwrap();
    assert_eq!(state.submit_count.load(Ordering::SeqCst), 2);
    assert_eq!(
        state.last_submit.lock().await.as_ref().unwrap()["nonce"],
        "8"
    );
}

#[rstest]
#[case("wrong_wallet")]
#[case("wrong_owner")]
#[case("undeployed")]
#[case("rpc_error")]
#[case("empty_identity")]
#[case("reverted_identity")]
#[tokio::test]
async fn test_wallet_validation_prevents_submission(#[case] failure: &str) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    match failure {
        "wrong_wallet" => *state.wallet.lock().await = TEST_SIGNER.into(),
        "wrong_owner" => *state.owner.lock().await = TEST_DEPOSIT_WALLET.into(),
        "undeployed" => {
            *state.rpc_code.lock().await = "0x".into();
            *state.rpc_identity.lock().await =
                Some(json!({"jsonrpc": "2.0", "id": 1, "result": "0x"}));
        }
        "empty_identity" => {
            *state.rpc_identity.lock().await =
                Some(json!({"jsonrpc": "2.0", "id": 1, "result": "0x"}));
        }
        "reverted_identity" => {
            *state.rpc_identity.lock().await = Some(
                json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32000, "message": "revert at https://rpc.example/dummy-provider-secret"}}),
            );
        }
        "rpc_error" => *state.rpc_error.lock().await = true,
        _ => unreachable!(),
    }

    let err = position_client(&addr)
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap_err();

    let expected = match failure {
        "wrong_wallet" => "not a canonical Deposit Wallet",
        "wrong_owner" => "signer is not its owner",
        "undeployed" => "not deployed",
        "empty_identity" => "Safe and Proxy wallets are unsupported",
        "reverted_identity" => "Deposit Wallet identity call failed",
        _ => "Polygon RPC",
    };

    assert!(err.to_string().contains(expected));
    assert!(!err.to_string().contains("dummy-provider-secret"));

    if failure == "reverted_identity" {
        assert!(err.to_string().contains("code -32000"));
    }

    assert_eq!(state.submit_count.load(Ordering::SeqCst), 0);
}

#[rstest]
#[tokio::test]
async fn test_wallet_validation_accepts_legacy_deposit_wallet() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    *state.wallet.lock().await = TEST_SIGNER.into();
    *state.legacy_wallet.lock().await = Some(wallet_address(&addr));
    position_client(&addr)
        .split_position(CONDITION_ID, dec!(1))
        .await
        .unwrap();
    assert_eq!(state.submit_count.load(Ordering::SeqCst), 1);
}

#[cfg(feature = "python")]
#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_python_transaction_id_survives_wait_failure_and_cancel(#[case] cancel: bool) {
    use pyo3::{
        Python,
        types::{PyDict, PyDictMethods, PyModule},
    };

    let state = TestServerState::default();
    *state.poll_statuses.lock().await = VecDeque::from([StatusCode::NOT_FOUND]);
    let addr = start_mock_server(state).await;
    let wallet = wallet_address(&addr);
    Python::initialize();
    Python::attach(|py| {
        static MODULE: std::sync::OnceLock<pyo3::Py<PyModule>> = std::sync::OnceLock::new();

        let module = MODULE.get_or_init(|| {
            let module = PyModule::new(py, "polymarket").unwrap();
            nautilus_polymarket::python::polymarket(py, &module).unwrap();
            module.unbind()
        });

        let locals = PyDict::new(py);
        locals.set_item("polymarket", module.bind(py)).unwrap();
        locals.set_item("private_key", TEST_PRIVATE_KEY).unwrap();
        locals.set_item("signer", TEST_SIGNER).unwrap();
        locals.set_item("wallet", wallet).unwrap();
        locals.set_item("url", format!("http://{addr}")).unwrap();
        locals.set_item("condition_id", CONDITION_ID).unwrap();
        locals.set_item("cancel", cancel).unwrap();
        let code = c"import asyncio
from decimal import Decimal
async def check():
    client = polymarket.PolymarketPositionClient(private_key=private_key, funder=wallet, relayer_api_key='dummy-key', relayer_api_key_address=signer, base_url_relayer=url, base_url_clob=url, base_url_rpc=url, timeout_secs=2)
    tx = await client.split_position(condition_id, Decimal('1'))
    assert tx.transaction_id == 'tx-1'
    future = tx.wait()
    if cancel:
        future.cancel()
    try:
        await future
    except (asyncio.CancelledError, ValueError):
        pass
    else:
        raise AssertionError('wait must fail or cancel')
    assert tx.transaction_id == 'tx-1'
    assert 'tx-1' in repr(tx)
    try:
        tx.wait()
    except ValueError:
        pass
    else:
        raise AssertionError('second wait must fail')
asyncio.run(check())";
        py.run(code, Some(&locals), Some(&locals)).unwrap();
    });
}
