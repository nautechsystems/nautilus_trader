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

use std::time::Duration;

use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
use nautilus_polymarket::{
    http::gamma::PolymarketGammaHttpClient, websocket::client::PolymarketWebSocketClient,
};
use rstest::rstest;

#[path = "../../../network/tests/integration/common/client_logging.rs"]
mod client_logging;

use client_logging::{REQUEST_TARGET, capture_logs, messages, start_server};

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_polymarket_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;
    let mut client =
        PolymarketWebSocketClient::new_market(Some(server.url.clone()), false, backend);
    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect().await.unwrap();
    })
    .await
    .unwrap();

    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    let logs = messages();
    client.disconnect().await.unwrap();
    server.task.abort();
    assert_eq!(requests, vec![REQUEST_TARGET.to_string(); 1]);
    assert!(debug.contains(&format!("url: {:?}", server.origin)));
    assert!(!debug.contains("URL_SECRET"));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|(level, message)| *level == log::Level::Debug
                && message.contains(&server.origin)
                && message.contains("connected"))
    );
}

#[rstest]
#[case("gamma_market_fee_crypto.json", false, Some(true), None, false)]
#[case("gamma_market_fee_crypto.json", true, Some(true), None, true)]
#[case("gamma_market_fee_zero_rate.json", false, Some(true), None, true)]
#[case(
    "gamma_market_fee_zero_rate.json",
    false,
    Some(true),
    Some("zero_fees"),
    false
)]
#[case(
    "gamma_market_fee_zero_rate.json",
    false,
    Some(true),
    Some(" ZERO_FEES "),
    false
)]
#[case(
    "gamma_market_fee_zero_rate.json",
    true,
    Some(true),
    Some("zero_fees"),
    false
)]
#[case(
    "gamma_market_fee_zero_rate.json",
    false,
    Some(true),
    Some("unknown_fees"),
    true
)]
#[case("gamma_market_fee_unclassifiable.json", false, Some(true), None, true)]
#[case("gamma_market_fee_unclassifiable.json", true, Some(true), None, true)]
#[case("gamma_market_fee_free.json", true, Some(false), None, false)]
#[case("gamma_market_fee_crypto.json", true, None, None, true)]
#[case("gamma_market_fee_crypto.json", true, Some(false), None, false)]
#[case("gamma_market_fee_unclassifiable.json", true, None, None, false)]
#[tokio::test]
async fn test_polymarket_missing_taker_rate_warning(
    #[case] filename: &str,
    #[case] missing_schedule: bool,
    #[case] fees_enabled: Option<bool>,
    #[case] fee_type: Option<&str>,
    #[case] warns: bool,
) {
    let _capture = capture_logs().await;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(filename);
    let mut market: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

    if missing_schedule {
        market["feeSchedule"] = serde_json::Value::Null;
    }

    market["feesEnabled"] = serde_json::to_value(fees_enabled).unwrap();
    if let Some(fee_type) = fee_type {
        market["feeType"] = fee_type.into();
    }

    let market_id = market["id"].as_str().unwrap().to_string();
    let response = serde_json::to_string(&vec![market]).unwrap();
    let router = axum::Router::new().route(
        "/markets",
        axum::routing::get(move || {
            let response = response.clone();
            async move { response }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = PolymarketGammaHttpClient::new(
        Some(format!("http://{address}")),
        5,
        RetryConfig::default(),
    )
    .unwrap();
    let instruments = client
        .request_instruments_by_slugs(vec!["fee-market".to_string()])
        .await
        .unwrap();
    server.abort();
    let warnings: Vec<_> = messages()
        .into_iter()
        .filter(|(_, message)| message.starts_with("Missing or zero Gamma taker fee rate"))
        .collect();

    let expected = if warns {
        vec![(
            log::Level::Warn,
            format!(
                "Missing or zero Gamma taker fee rate for fee-enabled market {market_id}, using zero"
            ),
        )]
    } else {
        vec![]
    };

    assert_eq!(instruments.len(), 2);
    assert_eq!(warnings, expected);
}
