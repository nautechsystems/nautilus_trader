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

use std::{collections::HashMap, sync::Arc};

use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, StatusCode, Uri},
    routing::get,
};
use nautilus_okx::{
    common::{
        credential::Credential,
        enums::{OKXContractType, OKXEnvironment, OKXInstrumentType, OKXMarginMode},
    },
    http::{
        client::OKXRawHttpClient,
        error::OKXHttpError,
        models::OKXAccountBill,
        query::{GetAccountBillsParams, GetAccountBillsParamsBuilder},
    },
};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Default)]
struct BillsServer {
    requests: tokio::sync::Mutex<Vec<(HeaderMap, Uri, Bytes)>>,
    response: tokio::sync::Mutex<Option<(StatusCode, String)>>,
}

fn bills_response() -> Value {
    serde_json::from_str::<Value>(include_str!("../../test_data/http_get_account_bills.json"))
        .unwrap()["response"]
        .clone()
}

async fn bills_client(
    state: Arc<BillsServer>,
    environment: OKXEnvironment,
    authenticated: bool,
) -> OKXRawHttpClient {
    let handler = get(move |headers: HeaderMap, uri: Uri, body: Bytes| {
        let state = state.clone();
        async move {
            state.requests.lock().await.push((headers, uri, body));
            state
                .response
                .lock()
                .await
                .clone()
                .unwrap_or_else(|| (StatusCode::OK, bills_response().to_string()))
        }
    });
    let router = Router::new()
        .route("/api/v5/account/bills", handler.clone())
        .route("/api/v5/account/bills-archive", handler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());

    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    if authenticated {
        OKXRawHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            "passphrase".to_string(),
            base_url,
            5,
            0,
            1,
            1,
            environment,
            None,
        )
        .unwrap()
    } else {
        OKXRawHttpClient::new(Some(base_url), 5, 0, 1, 1, environment, None).unwrap()
    }
}

async fn request_bills(
    client: &OKXRawHttpClient,
    archive: bool,
    params: GetAccountBillsParams,
) -> Result<Vec<OKXAccountBill>, OKXHttpError> {
    if archive {
        client.get_account_bills_archive(params).await
    } else {
        client.get_account_bills(params).await
    }
}

#[rstest]
#[tokio::test]
async fn test_account_bills_signed_single_page(
    #[values(false, true)] archive: bool,
    #[values(OKXEnvironment::Live, OKXEnvironment::Demo)] environment: OKXEnvironment,
    #[values(1_u32, 100_u32)] limit: u32,
) {
    let state = Arc::new(BillsServer::default());
    let client = bills_client(state.clone(), environment, true).await;
    let params = GetAccountBillsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Swap)
        .inst_id("BTC-USD-SWAP")
        .ccy("BTC")
        .mgn_mode(OKXMarginMode::Cross)
        .ct_type(OKXContractType::Inverse)
        .bill_type("2")
        .sub_type("174")
        .after("623950854533513220")
        .before("623950854533513217")
        .begin("1695033476000")
        .end("1695033477000")
        .limit(limit)
        .build()
        .unwrap();

    let bills = request_bills(&client, archive, params).await.unwrap();
    assert_eq!(
        serde_json::to_value(&bills).unwrap(),
        bills_response()["data"]
    );

    let requests = state.requests.lock().await;
    assert_eq!(requests.len(), 1);
    let (headers, uri, body) = &requests[0];
    let path = if archive {
        "/api/v5/account/bills-archive"
    } else {
        "/api/v5/account/bills"
    };
    assert_eq!(uri.path(), path);
    assert!(body.is_empty());
    let query: HashMap<String, String> = serde_urlencoded::from_str(uri.query().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(query).unwrap(),
        json!({
            "instType":"SWAP", "instId":"BTC-USD-SWAP", "ccy":"BTC",
            "mgnMode":"cross", "ctType":"inverse", "type":"2", "subType":"174",
            "after":"623950854533513220", "before":"623950854533513217",
            "begin":"1695033476000", "end":"1695033477000", "limit":limit.to_string()
        })
    );
    assert_eq!(headers["ok-access-key"], "test_key");
    assert_eq!(headers["ok-access-passphrase"], "passphrase");
    assert_eq!(
        headers
            .get("x-simulated-trading")
            .map(|v| v.to_str().unwrap()),
        (environment == OKXEnvironment::Demo).then_some("1")
    );
    let credential = Credential::new(
        "test_key".to_string(),
        "test_secret".to_string(),
        "passphrase".to_string(),
    );
    assert_eq!(
        headers["ok-access-sign"],
        credential.sign_bytes(
            headers["ok-access-timestamp"].to_str().unwrap(),
            "GET",
            uri.path_and_query().unwrap().as_str(),
            None,
        )
    );
}

#[rstest]
#[tokio::test]
async fn test_account_bills_optional_filters(
    #[values(false, true)] archive: bool,
    #[values(None, Some("USDT"))] ccy: Option<&str>,
) {
    let state = Arc::new(BillsServer::default());
    let client = bills_client(state.clone(), OKXEnvironment::Demo, true).await;
    let params = GetAccountBillsParams {
        ccy: ccy.map(String::from),
        ..Default::default()
    };
    let bills = request_bills(&client, archive, params).await.unwrap();

    assert_eq!(
        serde_json::to_value(bills).unwrap(),
        bills_response()["data"]
    );
    let requests = state.requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].1.query(), ccy.map(|_| "ccy=USDT"));
}

#[rstest]
#[tokio::test]
async fn test_account_bills_transfer_response(#[values(false, true)] archive: bool) {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../test_data/http_get_account_bills_transfer.json"
    ))
    .unwrap();
    let response = &fixture["response"];
    let state = Arc::new(BillsServer::default());
    *state.response.lock().await = Some((StatusCode::OK, response.to_string()));
    let client = bills_client(state.clone(), OKXEnvironment::Demo, true).await;

    let bills = request_bills(&client, archive, GetAccountBillsParams::default())
        .await
        .unwrap();

    assert_eq!(bills.len(), 3);
    for (bill, (ccy, amount)) in bills
        .iter()
        .zip([("USDC", "5000"), ("ETH", "1"), ("BTC", "1")])
    {
        assert_eq!(bill.ccy, ccy);
        assert_eq!(bill.bal, amount);
        assert_eq!(bill.bal_chg, amount);
        assert_eq!(bill.sz.as_deref(), Some(amount));
        assert_eq!(bill.bill_type, "1");
        assert_eq!(bill.sub_type, "11");
        assert_eq!(bill.from.as_deref(), Some("18"));
        assert_eq!(bill.to.as_deref(), Some("18"));
        assert_eq!(bill.inst_type.as_deref(), Some(""));
        assert_eq!(bill.ord_id.as_deref(), Some(""));
        assert_eq!(bill.pos_bal.as_deref(), Some(""));
        assert_eq!(bill.trade_id.as_deref(), Some("0"));
        assert_eq!(bill.earn_amt.as_deref(), Some(""));
        assert_eq!(bill.earn_apr.as_deref(), Some(""));
        assert_eq!(bill.notes.as_deref(), Some("From Unified account"));
    }
    assert_eq!(serde_json::to_value(bills).unwrap(), response["data"]);
    assert_eq!(state.requests.lock().await.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_account_bills_requires_credentials(#[values(false, true)] archive: bool) {
    let state = Arc::new(BillsServer::default());
    let client = bills_client(state.clone(), OKXEnvironment::Demo, false).await;

    assert!(matches!(
        request_bills(&client, archive, GetAccountBillsParams::default()).await,
        Err(OKXHttpError::MissingCredentials)
    ));
    assert!(state.requests.lock().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_account_bills_empty_page(#[values(false, true)] archive: bool) {
    let state = Arc::new(BillsServer::default());
    *state.response.lock().await = Some((
        StatusCode::OK,
        json!({"code":"0","msg":"","data":[]}).to_string(),
    ));
    let client = bills_client(state.clone(), OKXEnvironment::Demo, true).await;

    assert!(
        request_bills(&client, archive, GetAccountBillsParams::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(state.requests.lock().await.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_account_bills_exchange_error(
    #[values(false, true)] archive: bool,
    #[values(StatusCode::OK, StatusCode::UNAUTHORIZED)] status: StatusCode,
) {
    let state = Arc::new(BillsServer::default());
    *state.response.lock().await = Some((
        status,
        json!({"code":"50113","msg":"Invalid signature","data":[]}).to_string(),
    ));
    let client = bills_client(state, OKXEnvironment::Demo, true).await;

    match request_bills(&client, archive, GetAccountBillsParams::default()).await {
        Err(OKXHttpError::OkxError {
            error_code,
            message,
        }) => {
            assert_eq!(error_code, "50113");
            assert_eq!(message, "Invalid signature");
        }
        other => panic!("Expected OkxError, received {other:?}"),
    }
}

#[rstest]
#[case("", "empty")]
#[case("not JSON", "malformed")]
#[case(r#"{"code":"0","msg":""}"#, "decoding")]
#[case(r#"{"code":"0","msg":"","data":null}"#, "decoding")]
#[case(r#"{"code":"0","msg":"","data":[{}]}"#, "decoding")]
#[tokio::test]
async fn test_account_bills_invalid_response(
    #[values(false, true)] archive: bool,
    #[case] body: &str,
    #[case] error_kind: &str,
) {
    let state = Arc::new(BillsServer::default());
    *state.response.lock().await = Some((StatusCode::OK, body.to_string()));
    let client = bills_client(state, OKXEnvironment::Demo, true).await;
    let e = request_bills(&client, archive, GetAccountBillsParams::default())
        .await
        .unwrap_err();

    assert!(matches!(
        (error_kind, e),
        ("empty", OKXHttpError::EmptyResponse)
            | ("malformed", OKXHttpError::MalformedResponse(_))
            | ("decoding", OKXHttpError::ResponseDecoding(_))
    ));
}

#[rstest]
#[case("1", "11")]
#[case("999999", "999998")]
fn test_account_bill_synthetic_non_trade_schema(#[case] bill_type: &str, #[case] sub_type: &str) {
    // Synthetic schema cases exercise unknown codes, empty fields, and absent fields
    let value = json!({
        "billId":"18446744073709551616", "ccy":"USDT", "ts":"1695033476167",
        "bal":"8694.217940337829020200", "balChg":"-0.000000000000000001",
        "type":bill_type, "subType":sub_type, "fee":"", "ordId":"",
        "instType":"", "mgnMode":"", "from":"6", "to":"18"
    });
    let bill: OKXAccountBill = serde_json::from_value(value.clone()).unwrap();

    assert_eq!(bill.fee.as_deref(), Some(""));
    assert_eq!(bill.ord_id.as_deref(), Some(""));
    assert!(bill.trade_id.is_none());
    assert!(bill.pos_bal.is_none());
    assert_eq!(serde_json::to_value(bill).unwrap(), value);
}

#[rstest]
fn test_account_bill_requires_string_core_fields(
    #[values("billId", "ccy", "ts", "bal", "balChg", "type", "subType")] field: &str,
    #[values(None, Some(json!(1)), Some(Value::Null))] replacement: Option<Value>,
) {
    let mut value = bills_response()["data"][0].clone();

    if let Some(replacement) = replacement {
        value[field] = replacement;
    } else {
        value.as_object_mut().unwrap().remove(field);
    }

    assert!(serde_json::from_value::<OKXAccountBill>(value).is_err());
}
