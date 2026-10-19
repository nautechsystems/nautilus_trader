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
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{Router, http::Uri, routing::get};
use nautilus_common::{clients::DataClient, live::runner::set_data_event_sender};
use nautilus_core::string::urlencoding;
use nautilus_model::identifiers::ClientId;
use nautilus_tardis::{
    common::enums::TardisExchange,
    config::TardisDataClientConfig,
    data::TardisDataClient,
    http::TardisHttpClient,
    machine::{
        replay_normalized, stream_normalized,
        types::{ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions},
    },
};
use rstest::rstest;

#[path = "../../../network/tests/integration/common/client_logging.rs"]
mod client_logging;
use client_logging::{capture_logs, messages, start_server};

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_machine_connection_logging(#[case] replay: bool) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;
    let base_url = server.url.split('?').next().unwrap();
    let signal = Arc::new(AtomicBool::new(false));
    let options;

    if replay {
        let opts = vec![ReplayNormalizedRequestOptions {
            exchange: TardisExchange::Deribit,
            symbols: Some(vec!["BTC-PERPETUAL".to_string()]),
            from: "2026-10-01".parse().unwrap(),
            to: "2026-10-02".parse().unwrap(),
            data_types: vec!["trade".to_string()],
            with_disconnect_messages: Some(true),
        }];
        options = serde_json::to_string(&opts).unwrap();
        let _stream = tokio::time::timeout(
            Duration::from_secs(5),
            replay_normalized(base_url, opts, signal),
        )
        .await
        .unwrap()
        .unwrap();
    } else {
        let opts = vec![StreamNormalizedRequestOptions {
            exchange: TardisExchange::Deribit,
            symbols: Some(vec!["BTC-PERPETUAL".to_string()]),
            data_types: vec!["trade".to_string()],
            with_disconnect_messages: Some(true),
            timeout_interval_ms: Some(1500),
        }];
        options = serde_json::to_string(&opts).unwrap();
        let _stream = tokio::time::timeout(
            Duration::from_secs(5),
            stream_normalized(base_url, opts, signal),
        )
        .await
        .unwrap()
        .unwrap();
    }

    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    server.task.abort();

    let operation = if replay { "replay" } else { "stream" };
    assert_eq!(
        requests,
        vec![format!(
            "/URL_SECRET_PATH/ws-{operation}-normalized?options={}",
            urlencoding::encode(&options)
        )]
    );
    let logs = messages();
    assert!(logs.contains(&(
        log::Level::Debug,
        format!("Connecting to {}", server.origin)
    )));
    assert!(logs.contains(&(log::Level::Debug, format!("Connected to {}", server.origin))));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
}

#[tokio::test]
async fn test_http_and_data_connection_logging() {
    let _capture = capture_logs().await;
    let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new().route(
        "/URL_SECRET_PATH/instruments/deribit",
        get(move |uri: Uri| {
            let request_tx = request_tx.clone();
            async move {
                request_tx.send(uri.to_string()).unwrap();
                "[]"
            }
        }),
    );

    let http_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base_url = format!("http://{addr}/URL_SECRET_PATH");
    let http = TardisHttpClient::new(
        Some("URL_SECRET_API_KEY"),
        Some(&base_url),
        Some(2),
        true,
        None,
    )
    .unwrap();
    let instruments = http
        .instruments_info(TardisExchange::Deribit, None, None)
        .await
        .unwrap();
    assert!(instruments.is_empty());
    assert_eq!(
        request_rx.recv().await.unwrap(),
        "/URL_SECRET_PATH/instruments/deribit"
    );
    let server = start_server(1, None).await;
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    set_data_event_sender(sender);

    let mut client = TardisDataClient::new(
        ClientId::from("TARDIS"),
        TardisDataClientConfig {
            api_key: Some("URL_SECRET_API_KEY".into()),
            tardis_http_url: Some(base_url.into()),
            tardis_ws_url: Some(server.url.split('?').next().unwrap().into()),
            stream_options: vec![StreamNormalizedRequestOptions {
                exchange: TardisExchange::Deribit,
                symbols: None,
                data_types: vec!["trade".to_string()],
                with_disconnect_messages: None,
                timeout_interval_ms: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();

    tokio::time::timeout(Duration::from_secs(5), client.connect())
        .await
        .unwrap()
        .unwrap();
    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    let logs = messages();
    client.disconnect().await.unwrap();
    http_task.abort();
    server.task.abort();
    assert!(requests[0].starts_with("/URL_SECRET_PATH/ws-stream-normalized?options="));
    assert!(logs.contains(&(log::Level::Debug, format!("Requesting: http://{addr}"))));
    assert!(logs.contains(&(log::Level::Debug, format!("URL: {}", server.origin))));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
}

#[tokio::test]
async fn test_http_transport_error_omits_url_credentials() {
    let _capture = capture_logs().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://URL_SECRET_USER:URL_SECRET_PASSWORD@{addr}/URL_SECRET_PATH");
    let client =
        TardisHttpClient::new(Some("URL_SECRET_API_KEY"), Some(&url), Some(1), true, None).unwrap();
    let error = client
        .instruments_info(TardisExchange::Deribit, None, None)
        .await
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains("URL_SECRET"));
    assert!(error.to_string().contains(&format!("http://{addr}")));
    let logs = messages();
    assert!(logs.contains(&(log::Level::Debug, format!("Requesting: http://{addr}"))));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
}

#[rstest]
#[case("normal")]
#[case("abnormal")]
#[case("malformed")]
#[tokio::test]
async fn test_stream_provider_content_is_not_logged(#[case] kind: &str) {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    };
    let _capture = capture_logs().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");

    let message = if kind == "malformed" {
        Message::Text("URL_SECRET_BODY".into())
    } else {
        Message::Close(Some(CloseFrame {
            code: if kind == "normal" {
                CloseCode::Normal
            } else {
                CloseCode::Policy
            },
            reason: "URL_SECRET_CLOSE_REASON".into(),
        }))
    };

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
        socket.send(message).await.unwrap();
        socket.flush().await.unwrap();
    });

    let opts = vec![StreamNormalizedRequestOptions {
        exchange: TardisExchange::Deribit,
        symbols: None,
        data_types: vec!["trade".to_string()],
        with_disconnect_messages: None,
        timeout_interval_ms: None,
    }];
    let stream = stream_normalized(&url, opts, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    futures_util::pin_mut!(stream);
    let result = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap();
    server.await.unwrap();
    let logs = messages();
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );

    match kind {
        "normal" => {
            assert!(result.is_none());
            assert!(logs.contains(&(log::Level::Debug, "Connection closed normally".to_string())));
        }
        "abnormal" => {
            let error = result.unwrap().unwrap_err();
            assert!(error.to_string().contains("URL_SECRET_CLOSE_REASON"));
            assert!(logs.contains(&(
                log::Level::Warn,
                "Connection closed abnormally with code: Policy".to_string()
            )));
        }
        "malformed" => {
            assert!(result.unwrap().is_err());
            assert!(
                logs.iter()
                    .any(|(level, message)| *level == log::Level::Error
                        && message.starts_with("Failed to deserialize message at line 1, column "))
            );
        }
        _ => unreachable!(),
    }
}
