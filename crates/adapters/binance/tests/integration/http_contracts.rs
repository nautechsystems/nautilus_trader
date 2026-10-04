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
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    http::{HeaderMap, Method, StatusCode, Uri},
    routing::{any, get},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use nautilus_binance::{
    common::{
        consts::{BINANCE_NAUTILUS_FUTURES_BROKER_ID, BINANCE_NAUTILUS_SPOT_BROKER_ID},
        credential::SigningCredential,
        encoder::encode_broker_id,
        enums::{BinanceEnvironment, BinanceProductType, BinanceSide, BinanceTimeInForce},
    },
    futures::http::{
        client::{BinanceFuturesHttpClient, BinanceRawFuturesHttpClient},
        error::BinanceFuturesHttpError,
        models::BatchOrderResult,
        query::{
            BatchCancelItem, BatchModifyItem, BinanceAlgoOrderQueryParams, BinanceCancelOrderParams,
        },
    },
    spot::{
        enums::BinanceSpotOrderType,
        http::{client::BinanceRawSpotHttpClient, error::BinanceSpotHttpError},
    },
};
use nautilus_model::{identifiers::ClientOrderId, instruments::Instrument, types::Price};
use parking_lot::Mutex;
use rstest::rstest;

#[rstest]
#[case::spot(BinanceProductType::Spot)]
#[case::usdm(BinanceProductType::UsdM)]
#[case::coinm(BinanceProductType::CoinM)]
#[tokio::test]
async fn test_signed_query_assembly(
    #[case] product: BinanceProductType,
    #[values(false, true)] ed25519: bool,
) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();
    let app = Router::new().fallback(get(move |uri: Uri, headers: HeaderMap| {
        let requests = requests.clone();
        async move {
            requests.lock().push((uri, headers));
            include_str!("../../test_data/spot/http_json/ping_response.json")
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let secret = if ed25519 {
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        der.extend_from_slice(&[0xABu8; 32]);
        STANDARD.encode(der)
    } else {
        "test-secret".to_string()
    };
    let credential = SigningCredential::new("test-key".to_string(), secret.clone());
    let params = [("symbol", "BTCUSDT"), ("origClientOrderId", "a b+/=")];
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    if product == BinanceProductType::Spot {
        let client = BinanceRawSpotHttpClient::new_with_json_responses(
            BinanceEnvironment::Live,
            Some("test-key".to_string()),
            Some(secret),
            Some(url),
            Some(4321),
            Some(5),
            None,
            true,
        )
        .unwrap();
        client.get_signed("order", Some(&params)).await.unwrap();
    } else {
        let client = BinanceRawFuturesHttpClient::new(
            product,
            BinanceEnvironment::Live,
            Some("test-key".to_string()),
            Some(secret),
            Some(url),
            Some(4321),
            Some(5),
            None,
        )
        .unwrap();
        client
            .get::<_, serde_json::Value>("order", Some(&params), true, false)
            .await
            .unwrap();
    }
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    server.abort();
    let requests = captured.lock();
    assert_eq!(requests.len(), 1);
    let (uri, headers) = &requests[0];
    let (signed, signature) = uri.query().unwrap().rsplit_once("&signature=").unwrap();
    let prefix = "symbol=BTCUSDT&origClientOrderId=a+b%2B%2F%3D&timestamp=";
    let timestamp = signed
        .strip_prefix(prefix)
        .unwrap()
        .strip_suffix("&recvWindow=4321")
        .unwrap()
        .parse::<u128>()
        .unwrap();
    let expected_signature =
        serde_urlencoded::to_string([("signature", credential.sign(signed))]).unwrap();

    assert!(timestamp >= before && timestamp <= after);
    assert_eq!(headers.get("X-MBX-APIKEY").unwrap(), "test-key");
    assert_eq!(format!("signature={signature}"), expected_signature);
    assert_eq!(credential.is_ed25519(), ed25519);
    assert_eq!(
        uri.path(),
        match product {
            BinanceProductType::Spot => "/api/v3/order",
            BinanceProductType::UsdM => "/fapi/v1/order",
            BinanceProductType::CoinM => "/dapi/v1/order",
            _ => unreachable!(),
        }
    );
}

#[rstest]
#[case::usdm(BinanceProductType::UsdM)]
#[case::coinm(BinanceProductType::CoinM)]
#[tokio::test]
async fn test_instrument_requests_refresh_venue_values(#[case] product: BinanceProductType) {
    let fixture = match product {
        BinanceProductType::UsdM => {
            include_str!("../../test_data/futures/http_json/exchange_info_usdm.json")
        }
        BinanceProductType::CoinM => {
            include_str!("../../test_data/futures/http_json/exchange_info_delivery_coinm.json")
        }
        _ => unreachable!(),
    };
    let response: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let requests = Arc::new(Mutex::new(0));
    let captured = requests.clone();
    let app = Router::new().fallback(get(move || {
        let captured = captured.clone();
        let mut response = response.clone();
        async move {
            let count = {
                let mut count = captured.lock();
                *count += 1;
                *count
            };
            let symbols = response["symbols"].as_array_mut().unwrap();
            symbols.truncate(1);
            let filter = symbols[0]["filters"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|filter| filter["filterType"] == "PRICE_FILTER")
                .unwrap();
            filter["tickSize"] =
                serde_json::Value::String(if count == 1 { "0.01" } else { "0.10" }.to_string());
            axum::Json(response)
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let clock = nautilus_core::time::get_atomic_clock_realtime();
    let client = BinanceFuturesHttpClient::new(
        product,
        BinanceEnvironment::Live,
        clock,
        None,
        None,
        Some(url),
        None,
        Some(5),
        None,
        false,
    )
    .unwrap();
    let first = client.request_instruments().await.unwrap();
    let second = client.request_instruments().await.unwrap();
    server.abort();

    assert_eq!(*requests.lock(), 2);
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].id(), second[0].id());
    assert_eq!(first[0].price_increment(), Price::from("0.01"));
    assert_eq!(second[0].price_increment(), Price::from("0.10"));
}

#[rstest]
#[case::historical(false, true, None, false, true)]
#[case::current(true, false, None, false, true)]
#[case::ambiguous(true, true, None, false, false)]
#[case::missing(false, false, None, false, false)]
#[case::auth(false, true, Some(-2015), false, false)]
#[case::known_venue_id(true, true, None, true, true)]
#[tokio::test]
async fn test_encoded_order_cancel_resolves_historical_identity(
    #[values((BinanceProductType::Spot, false), (BinanceProductType::UsdM, false), (BinanceProductType::UsdM, true))]
    venue: (BinanceProductType, bool),
    #[case] current_exists: bool,
    #[case] historical_exists: bool,
    #[case] lookup_error: Option<i64>,
    #[case] known_id: bool,
    #[case] expected_success: bool,
) {
    let (product, algo) = venue;

    let client_key = if algo {
        "clientAlgoId"
    } else {
        "origClientOrderId"
    };

    let order_key = if algo { "algoId" } else { "orderId" };
    let original = "O-20260922-160119-V2-000-8";

    let broker = if product == BinanceProductType::Spot {
        BINANCE_NAUTILUS_SPOT_BROKER_ID
    } else {
        BINANCE_NAUTILUS_FUTURES_BROKER_ID
    };

    let encoded = encode_broker_id(&ClientOrderId::new(original), broker);
    let wire = encoded.clone();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();

    let app = Router::new().fallback(any(move |method: Method, uri: Uri| {
        let requests = requests.clone();
        let wire = wire.clone();
        async move {
            let query: std::collections::HashMap<String, String> =
                serde_urlencoded::from_str(uri.query().unwrap()).unwrap();
            requests.lock().push((method.clone(), query.clone()));
            let current = query.get(client_key) == Some(&wire);

            let exists = if current {
                current_exists
            } else {
                historical_exists
            };

            let error = lookup_error.or_else(|| (!exists).then_some(-2013));

            if method == Method::GET
                && let Some(code) = error
            {
                let mut value: serde_json::Value = serde_json::from_str(include_str!(
                    "../../test_data/spot/http_json/ping_response.json"
                ))
                .unwrap();
                value["code"] = code.into();
                value["msg"] = "Lookup rejected".into();
                return (StatusCode::BAD_REQUEST, axum::Json(value));
            }

            let fixture = if algo {
                include_str!("../../test_data/futures/http_json/algo_order_response.json")
            } else {
                match (product, method == Method::DELETE) {
                    (BinanceProductType::Spot, true) => {
                        include_str!("../../test_data/spot/http_json/cancel_order_response.json")
                    }
                    (BinanceProductType::Spot, false) => {
                        include_str!("../../test_data/spot/http_json/order_response.json")
                    }
                    _ => include_str!("../../test_data/futures/http_json/order_response.json"),
                }
            };

            let mut value: serde_json::Value = serde_json::from_str(fixture).unwrap();
            value[order_key] = if current { 101 } else { 202 }.into();
            value[if algo {
                "clientAlgoId"
            } else {
                "clientOrderId"
            }] = if current { wire } else { original.to_string() }.into();

            if algo {
                value["code"] = "200".into();
                value["msg"] = "success".into();
            }

            (StatusCode::OK, axum::Json(value))
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let order_id = known_id.then_some(303);

    let success = if product == BinanceProductType::Spot {
        let client = BinanceRawSpotHttpClient::new_with_json_responses(
            BinanceEnvironment::Live,
            Some("test-key".to_string()),
            Some("test-secret".to_string()),
            Some(url),
            Some(4321),
            Some(5),
            None,
            true,
        )
        .unwrap();
        client
            .cancel_order("BTCUSDT", order_id, Some(&encoded))
            .await
            .is_ok()
    } else {
        let client = BinanceRawFuturesHttpClient::new(
            product,
            BinanceEnvironment::Live,
            Some("test-key".to_string()),
            Some("test-secret".to_string()),
            Some(url),
            Some(4321),
            Some(5),
            None,
        )
        .unwrap();

        if algo {
            client
                .cancel_algo_order(&BinanceAlgoOrderQueryParams {
                    algo_id: order_id,
                    client_algo_id: Some(encoded.clone()),
                    recv_window: None,
                })
                .await
                .is_ok()
        } else {
            client
                .cancel_order(&BinanceCancelOrderParams {
                    symbol: "BTCUSDT".to_string(),
                    order_id,
                    orig_client_order_id: Some(encoded.clone()),
                    recv_window: None,
                })
                .await
                .is_ok()
        }
    };

    server.abort();
    let requests = captured.lock();

    let expected_queries = if known_id {
        0
    } else if lookup_error.is_some() {
        1
    } else {
        2
    };

    assert_eq!(success, expected_success);
    assert_eq!(
        requests.len(),
        expected_queries + usize::from(expected_success)
    );

    if !known_id {
        assert_eq!(requests[0].0, Method::GET);
        assert_eq!(requests[0].1.get(client_key).unwrap(), &encoded);

        if expected_queries == 2 {
            assert_eq!(requests[1].0, Method::GET);
            assert_eq!(requests[1].1.get(client_key).unwrap(), original);
        }
    }

    if expected_success {
        let (method, query) = requests.last().unwrap();

        let expected_id = if known_id {
            "303"
        } else if current_exists {
            "101"
        } else {
            "202"
        };

        assert_eq!(*method, Method::DELETE);
        assert_eq!(query.get(order_key).unwrap(), expected_id);
        assert_eq!(query.get(client_key), None);
    }
}

#[rstest]
#[tokio::test]
async fn test_encoded_batch_preserves_missing_order_result(#[values(false, true)] modify: bool) {
    let missing = ClientOrderId::new("O-20260922-160119-V2-000-8");
    let present = ClientOrderId::new("O-20260922-160119-V2-000-9");
    let encoded_missing = encode_broker_id(&missing, BINANCE_NAUTILUS_FUTURES_BROKER_ID);
    let encoded_present = encode_broker_id(&present, BINANCE_NAUTILUS_FUTURES_BROKER_ID);
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();

    let app = Router::new().fallback(any(move |method: Method, uri: Uri| {
        let requests = requests.clone();
        async move {
            let query: std::collections::HashMap<String, String> =
                serde_urlencoded::from_str(uri.query().unwrap()).unwrap();
            requests.lock().push((method.clone(), query.clone()));
            let mut value: serde_json::Value = serde_json::from_str(include_str!(
                "../../test_data/futures/http_json/order_response.json"
            ))
            .unwrap();
            value["orderId"] = 202.into();
            value["clientOrderId"] = present.as_str().into();

            if method != Method::GET {
                return (
                    StatusCode::OK,
                    axum::Json(serde_json::Value::Array(vec![value])),
                );
            }

            if query.get("origClientOrderId").map(String::as_str) == Some(present.as_str()) {
                return (StatusCode::OK, axum::Json(value));
            }

            let mut error: serde_json::Value = serde_json::from_str(include_str!(
                "../../test_data/spot/http_json/ping_response.json"
            ))
            .unwrap();
            error["code"] = (-2013).into();
            error["msg"] = "Order does not exist".into();
            (StatusCode::BAD_REQUEST, axum::Json(error))
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BinanceRawFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
        Some(url),
        Some(4321),
        Some(5),
        None,
    )
    .unwrap();

    let result = if modify {
        let items = [encoded_missing, encoded_present].map(|id| BatchModifyItem {
            symbol: "BTCUSDT".to_string(),
            order_id: None,
            orig_client_order_id: Some(id),
            side: "BUY".to_string(),
            quantity: "0.002".to_string(),
            price: "42000".to_string(),
        });

        client.batch_modify_orders(&items).await.unwrap()
    } else {
        client
            .batch_cancel_orders(&[
                BatchCancelItem::by_client_order_id("BTCUSDT", encoded_missing),
                BatchCancelItem::by_client_order_id("BTCUSDT", encoded_present),
            ])
            .await
            .unwrap()
    };

    server.abort();
    assert_eq!(result.len(), 2);

    match &result[0] {
        BatchOrderResult::Error(error) => {
            assert_eq!(error.code, -2013);
            assert_eq!(error.msg, "Order does not exist");
        }
        other => panic!("Expected missing-order error, received {other:?}"),
    }

    match &result[1] {
        BatchOrderResult::Success(order) => assert_eq!(order.order_id, 202),
        other => panic!("Expected successful sibling, received {other:?}"),
    }

    let requests = captured.lock();
    assert_eq!(requests.len(), 5);

    if modify {
        assert_eq!(requests[4].0, Method::PUT);
        let orders: serde_json::Value =
            serde_json::from_str(requests[4].1.get("batchOrders").unwrap()).unwrap();
        assert_eq!(
            orders,
            serde_json::json!([{
                "symbol": "BTCUSDT", "orderId": "202", "side": "BUY", "quantity": "0.002", "price": "42000",
            }])
        );
    } else {
        assert_eq!(requests[4].0, Method::DELETE);
        assert_eq!(requests[4].1.get("orderIdList").unwrap(), "[202]");
        assert_eq!(requests[4].1.get("origClientOrderIdList"), None);
    }
}

#[rstest]
#[case::historical(true, false, false)]
#[case::current(false, false, false)]
#[case::different_replacement(true, true, false)]
#[case::lookup_rejected(true, false, true)]
#[tokio::test]
async fn test_encoded_spot_replace_retains_wire_identity(
    #[case] historical: bool,
    #[case] different_replacement: bool,
    #[case] lookup_rejected: bool,
) {
    let original = ClientOrderId::new("O-20260922-160119-V2-000-8");
    let encoded = encode_broker_id(&original, BINANCE_NAUTILUS_SPOT_BROKER_ID);

    let wire = if historical {
        original.to_string()
    } else {
        encoded.clone()
    };

    let replacement = if different_replacement {
        encode_broker_id(
            &ClientOrderId::new("O-20260922-160119-V2-000-9"),
            BINANCE_NAUTILUS_SPOT_BROKER_ID,
        )
    } else {
        encoded.clone()
    };

    let queried_wire = wire.clone();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let requests = captured.clone();

    let app = Router::new().fallback(any(move |method: Method, uri: Uri| {
        let requests = requests.clone();
        let wire = queried_wire.clone();
        async move {
            let query: std::collections::HashMap<String, String> =
                serde_urlencoded::from_str(uri.query().unwrap()).unwrap();
            requests.lock().push((method.clone(), query.clone()));

            if lookup_rejected {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({"code": -2015, "msg": "Rejected"})),
                );
            }

            let mut order: serde_json::Value = serde_json::from_str(include_str!(
                "../../test_data/spot/http_json/order_response.json"
            ))
            .unwrap();
            order["orderId"] = 101.into();
            order["clientOrderId"] = wire.into();

            if method == Method::POST {
                order["orderId"] = 202.into();
                order["clientOrderId"] = query["newClientOrderId"].clone().into();
                return (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({"newOrderResponse": order})),
                );
            }

            (StatusCode::OK, axum::Json(order))
        }
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BinanceRawSpotHttpClient::new_with_json_responses(
        BinanceEnvironment::Live,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
        Some(url),
        Some(4321),
        Some(5),
        None,
        true,
    )
    .unwrap();
    let result = client
        .cancel_replace_order(
            "BTCUSDT",
            BinanceSide::Buy,
            BinanceSpotOrderType::Limit,
            Some(BinanceTimeInForce::Gtc),
            Some("0.002"),
            Some("42000"),
            Some(101),
            Some(&encoded),
            Some("CR-test"),
            Some(&replacement),
        )
        .await;
    server.abort();
    let requests = captured.lock();
    assert_eq!(requests.len(), if lookup_rejected { 1 } else { 2 });
    assert_eq!(requests[0].0, Method::GET);
    assert_eq!(requests[0].1.get("orderId").unwrap(), "101");
    assert_eq!(requests[0].1.get("origClientOrderId"), None);

    if lookup_rejected {
        assert!(
            matches!(result.unwrap_err(), BinanceSpotHttpError::ValidationError(message) if message.contains("before submission"))
        );
    } else {
        let expected_wire = if different_replacement {
            &replacement
        } else {
            &wire
        };

        let order = result.unwrap();
        assert_eq!(order.order_id, 202);
        assert_eq!(&order.client_order_id, expected_wire);
        assert_eq!(requests[1].0, Method::POST);
        assert_eq!(
            requests[1].1.get("newClientOrderId").unwrap(),
            expected_wire
        );
        assert_eq!(requests[1].1.get("cancelOrderId").unwrap(), "101");
        assert_eq!(requests[1].1.get("cancelOrigClientOrderId"), None);
        assert_eq!(
            requests[1].1.get("cancelNewClientOrderId").unwrap(),
            "CR-test"
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_algo_cancel_success_status_preserves_venue_error(
    #[values(-2011, -2013, -2015)] code: i64,
) {
    let app = Router::new().fallback(any(move || async move {
        axum::Json(serde_json::json!({"algoId": 123456789_i64, "clientAlgoId": "algo-test", "code": code.to_string(), "msg": "cancel rejected"}))
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());

    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = BinanceFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        nautilus_core::time::get_atomic_clock_realtime(),
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
        Some(url),
        Some(4321),
        Some(5),
        None,
        false,
    )
    .unwrap();
    let error = client
        .cancel_algo_order(ClientOrderId::new("algo-test"))
        .await
        .unwrap_err();
    server.abort();

    let Some(BinanceFuturesHttpError::BinanceError {
        code: actual,
        message,
        status,
        retry_after,
    }) = error.downcast_ref::<BinanceFuturesHttpError>()
    else {
        panic!("Expected typed venue error: {error}");
    };

    assert_eq!(*actual, code);
    assert_eq!(message, "cancel rejected");
    assert_eq!(*status, 200);
    assert_eq!(*retry_after, None);
}
