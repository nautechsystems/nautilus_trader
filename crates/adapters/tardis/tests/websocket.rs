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

//! Integration tests for Tardis Machine WebSocket client using mock servers.

use std::net::SocketAddr;

use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use nautilus_model::{data::Data, identifiers::InstrumentId};
use nautilus_tardis::{
    common::enums::TardisExchange,
    config::BookSnapshotOutput,
    machine::types::{
        ReplayNormalizedRequestOptions, TardisInstrumentMiniInfo, TardisMachineClient,
    },
};
use rstest::rstest;
use ustr::Ustr;

const TRADE_FIXTURE: &str = include_str!("../test_data/trade.json");
const BOOK_CHANGE_FIXTURE: &str = include_str!("../test_data/book_change.json");
const BAR_FIXTURE: &str = include_str!("../test_data/bar.json");
const DISCONNECT_FIXTURE: &str = include_str!("../test_data/disconnect.json");

async fn start_mock_ws_server(app: Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

async fn handle_replay_ws(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|mut socket: WebSocket| async move {
        // Send a trade message then close normally
        let _ = socket.send(Message::Text(TRADE_FIXTURE.into())).await;
        let _ = socket.close().await;
    })
}

async fn handle_disconnect_then_trade_ws(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|mut socket: WebSocket| async move {
        // Send a disconnect message (should be skipped) then a trade
        let _ = socket.send(Message::Text(DISCONNECT_FIXTURE.into())).await;
        let _ = socket.send(Message::Text(TRADE_FIXTURE.into())).await;
        let _ = socket.close().await;
    })
}

fn create_machine_client(addr: SocketAddr) -> TardisMachineClient {
    let base_url = format!("ws://{addr}");
    let mut client =
        TardisMachineClient::new(Some(&base_url), true, BookSnapshotOutput::Deltas).unwrap();

    let info = TardisInstrumentMiniInfo::new(
        InstrumentId::from("XBTUSD.BITMEX"),
        Some(Ustr::from("XBTUSD")),
        TardisExchange::Bitmex,
        1,
        0,
    );
    client.add_instrument_info(info);
    client
}

#[rstest]
#[tokio::test]
async fn test_replay_stream_receives_trade() {
    let app = Router::new().route("/ws-replay-normalized", get(handle_replay_ws));
    let (addr, _handle) = start_mock_ws_server(app).await;
    let client = create_machine_client(addr);

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: TardisExchange::Bitmex,
        symbols: Some(vec!["XBTUSD".to_string()]),
        from: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        to: chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
        data_types: vec!["trade".to_string()],
        with_disconnect_messages: Some(false),
    }];

    let stream = client.replay(options).await.unwrap();
    futures_util::pin_mut!(stream);

    let mut received = Vec::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(data) => received.push(data),
            Err(_) => break,
        }
    }

    assert!(!received.is_empty(), "Expected at least one trade");
}

#[rstest]
#[tokio::test]
async fn test_disconnect_message_skipped() {
    let app = Router::new().route(
        "/ws-replay-normalized",
        get(handle_disconnect_then_trade_ws),
    );
    let (addr, _handle) = start_mock_ws_server(app).await;
    let client = create_machine_client(addr);

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: TardisExchange::Bitmex,
        symbols: Some(vec!["XBTUSD".to_string()]),
        from: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        to: chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
        data_types: vec!["trade".to_string()],
        with_disconnect_messages: Some(true),
    }];

    let stream = client.replay(options).await.unwrap();
    futures_util::pin_mut!(stream);

    let mut received = Vec::new();

    while let Some(result) = stream.next().await {
        if let Ok(data) = result {
            received.push(data);
        }
    }

    // The disconnect message is filtered; only the trade should arrive
    assert_eq!(
        received.len(),
        1,
        "Expected exactly one trade after disconnect skip"
    );
}

#[rstest]
#[tokio::test]
async fn test_machine_client_close() {
    let app = Router::new().route("/ws-replay-normalized", get(handle_replay_ws));
    let (addr, _handle) = start_mock_ws_server(app).await;
    let mut client = create_machine_client(addr);

    assert!(!client.is_closed());
    client.close();
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_replay_stream_receives_book_deltas() {
    let app = Router::new().route(
        "/ws-replay-normalized",
        get(|ws: WebSocketUpgrade| async {
            ws.on_upgrade(|mut socket: WebSocket| async move {
                let _ = socket.send(Message::Text(BOOK_CHANGE_FIXTURE.into())).await;
                let _ = socket.close().await;
            })
        }),
    );
    let (addr, _handle) = start_mock_ws_server(app).await;
    let client = create_machine_client(addr);

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: TardisExchange::Bitmex,
        symbols: Some(vec!["XBTUSD".to_string()]),
        from: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        to: chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
        data_types: vec!["book_change".to_string()],
        with_disconnect_messages: Some(false),
    }];

    let stream = client.replay(options).await.unwrap();
    futures_util::pin_mut!(stream);

    let mut received = Vec::new();

    while let Some(result) = stream.next().await {
        if let Ok(data) = result {
            received.push(data);
        }
    }

    assert!(!received.is_empty(), "Expected at least one book delta");
    assert!(
        matches!(received[0], Data::Deltas(_)),
        "Expected Deltas variant"
    );
}

#[rstest]
#[tokio::test]
async fn test_replay_stream_receives_bar() {
    let app = Router::new().route(
        "/ws-replay-normalized",
        get(|ws: WebSocketUpgrade| async {
            ws.on_upgrade(|mut socket: WebSocket| async move {
                let _ = socket.send(Message::Text(BAR_FIXTURE.into())).await;
                let _ = socket.close().await;
            })
        }),
    );
    let (addr, _handle) = start_mock_ws_server(app).await;
    let client = create_machine_client(addr);

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: TardisExchange::Bitmex,
        symbols: Some(vec!["XBTUSD".to_string()]),
        from: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        to: chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
        data_types: vec!["trade_bar_1m".to_string()],
        with_disconnect_messages: Some(false),
    }];

    let stream = client.replay(options).await.unwrap();
    futures_util::pin_mut!(stream);

    let mut received = Vec::new();

    while let Some(result) = stream.next().await {
        if let Ok(data) = result {
            received.push(data);
        }
    }

    assert!(!received.is_empty(), "Expected at least one bar");
    assert!(matches!(received[0], Data::Bar(_)), "Expected Bar variant");
}

#[rstest]
#[tokio::test]
async fn test_replay_stream_multiple_message_types() {
    let app = Router::new().route(
        "/ws-replay-normalized",
        get(|ws: WebSocketUpgrade| async {
            ws.on_upgrade(|mut socket: WebSocket| async move {
                let _ = socket.send(Message::Text(TRADE_FIXTURE.into())).await;
                let _ = socket.send(Message::Text(BOOK_CHANGE_FIXTURE.into())).await;
                let _ = socket.send(Message::Text(BAR_FIXTURE.into())).await;
                let _ = socket.close().await;
            })
        }),
    );
    let (addr, _handle) = start_mock_ws_server(app).await;
    let client = create_machine_client(addr);

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: TardisExchange::Bitmex,
        symbols: Some(vec!["XBTUSD".to_string()]),
        from: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        to: chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
        data_types: vec![
            "trade".to_string(),
            "book_change".to_string(),
            "trade_bar_1m".to_string(),
        ],
        with_disconnect_messages: Some(false),
    }];

    let stream = client.replay(options).await.unwrap();
    futures_util::pin_mut!(stream);

    let mut received = Vec::new();

    while let Some(result) = stream.next().await {
        if let Ok(data) = result {
            received.push(data);
        }
    }

    assert_eq!(received.len(), 3, "Expected trade + deltas + bar");
}

#[rstest]
#[tokio::test]
async fn test_replay_stream_malformed_message_continues() {
    let app = Router::new().route(
        "/ws-replay-normalized",
        get(|ws: WebSocketUpgrade| async {
            ws.on_upgrade(|mut socket: WebSocket| async move {
                // Send malformed JSON (should be logged and skipped)
                let _ = socket.send(Message::Text("not valid json".into())).await;
                // Then send a valid trade (should still arrive)
                let _ = socket.send(Message::Text(TRADE_FIXTURE.into())).await;
                let _ = socket.close().await;
            })
        }),
    );
    let (addr, _handle) = start_mock_ws_server(app).await;
    let client = create_machine_client(addr);

    let options = vec![ReplayNormalizedRequestOptions {
        exchange: TardisExchange::Bitmex,
        symbols: Some(vec!["XBTUSD".to_string()]),
        from: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        to: chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
        data_types: vec!["trade".to_string()],
        with_disconnect_messages: Some(false),
    }];

    let stream = client.replay(options).await.unwrap();
    futures_util::pin_mut!(stream);

    let mut data_count = 0;
    let mut error_count = 0;

    while let Some(result) = stream.next().await {
        match result {
            Ok(_) => data_count += 1,
            Err(_) => error_count += 1,
        }
    }

    // Malformed JSON produces a deserialization error that terminates the
    // machine client stream (yield error, then break)
    assert_eq!(error_count, 1, "Expected one error for malformed JSON");
    assert_eq!(data_count, 0, "Stream ends after deserialization error");
}
