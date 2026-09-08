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
    http::{HeaderMap, Uri},
    routing::get,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use nautilus_binance::{
    common::{
        credential::SigningCredential,
        enums::{BinanceEnvironment, BinanceProductType},
    },
    futures::http::client::{BinanceFuturesHttpClient, BinanceRawFuturesHttpClient},
    spot::http::client::BinanceRawSpotHttpClient,
};
use nautilus_model::{instruments::Instrument, types::Price};
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
