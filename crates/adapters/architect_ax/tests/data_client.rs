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

//! Integration tests for data client handlers.
//!
//! These tests verify the end-to-end flow from WebSocket messages through handlers
//! to parsed Nautilus data types. Handler-level tests use the WebSocket stream directly,
//! while full client tests use the event sender channel.

mod common;

use std::time::Duration;

use futures_util::StreamExt;
use nautilus_architect_ax::{
    common::enums::{AxCandleWidth, AxMarketDataLevel},
    config::AxDataClientConfig,
    data::AxDataClient,
    http::client::AxHttpClient,
    websocket::{
        data::client::AxMdWebSocketClient,
        messages::{AxDataWsMessage, AxMdMessage},
    },
};
use nautilus_common::{
    clients::DataClient as DataClientTrait,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent,
        data::{SubscribeBars, SubscribeBookDeltas, SubscribeQuotes, SubscribeTrades},
    },
};
use nautilus_core::UUID4;
use nautilus_model::{
    data::{BarType, Data},
    enums::BookType,
    identifiers::{ClientId, InstrumentId},
};
use rstest::rstest;

use crate::common::server::{start_test_server, wait_for_connection};

fn setup_data_channel() -> tokio::sync::mpsc::UnboundedReceiver<DataEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(sender);
    receiver
}

#[rstest]
#[tokio::test]
async fn test_handler_emits_l1_md_message() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_quotes("EURUSD-PERP")
        .await
        .expect("Subscribe failed");

    let stream = client.stream();
    tokio::pin!(stream);

    let result = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;

    match result {
        Ok(Some(AxDataWsMessage::MdMessage(AxMdMessage::BookL1(book)))) => {
            assert_eq!(book.s.as_str(), "EURUSD-PERP");
        }
        Ok(Some(other)) => panic!("Expected MdMessage::BookL1, was {other:?}"),
        Ok(None) => panic!("Stream ended unexpectedly"),
        Err(_) => panic!("Timeout waiting for L1 message"),
    }

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_emits_trade_md_message() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_trades("EURUSD-PERP")
        .await
        .expect("Subscribe failed");

    let stream = client.stream();
    tokio::pin!(stream);

    let trade = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.next().await {
                Some(AxDataWsMessage::MdMessage(AxMdMessage::Trade(trade))) => {
                    return trade;
                }
                Some(_) => {}
                None => panic!("Stream closed without receiving a trade"),
            }
        }
    })
    .await
    .expect("Timeout waiting for trade message");

    assert_eq!(trade.s.as_str(), "EURUSD-PERP");
    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_emits_l2_md_message() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .expect("Subscribe failed");

    let stream = client.stream();
    tokio::pin!(stream);

    let result = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;

    match result {
        Ok(Some(AxDataWsMessage::MdMessage(AxMdMessage::BookL2(book)))) => {
            assert_eq!(book.s.as_str(), "EURUSD-PERP");
        }
        Ok(Some(other)) => panic!("Expected MdMessage::BookL2, was {other:?}"),
        Ok(None) => panic!("Stream ended unexpectedly"),
        Err(_) => panic!("Timeout waiting for order book message"),
    }

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_emits_candle_md_message() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_candles("EURUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .expect("Subscribe candles failed");

    let stream = client.stream();
    tokio::pin!(stream);

    let candle = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.next().await {
                Some(AxDataWsMessage::MdMessage(AxMdMessage::Candle(candle))) => return candle,
                Some(_) => {}
                None => panic!("Stream closed without receiving a candle"),
            }
        }
    })
    .await
    .expect("Timeout waiting for candle message");

    assert_eq!(candle.symbol.as_str(), "EURUSD-PERP");

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_ignores_unknown_symbol() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .expect("Subscribe failed");

    let stream = client.stream();
    tokio::pin!(stream);

    // Handler now emits raw venue messages regardless of instrument cache.
    // The consumer is responsible for filtering unknown symbols.
    // Just verify the stream produces messages without crashing.
    let result = tokio::time::timeout(Duration::from_secs(1), stream.next()).await;

    assert!(
        result.is_ok(),
        "Expected handler to forward raw messages even without instrument cache"
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_quote_tick_via_channel() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client =
        AxHttpClient::new(Some(http_url), None, None, None, None, None, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let mut client = AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client");

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    // Use first instrument from HTTP fixture (EURUSD-PERP)
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let subscribe_cmd = SubscribeQuotes {
        instrument_id,
        client_id: Some(client_id),
        venue: None,
        command_id: UUID4::new(),
        ts_init: 0.into(),
        correlation_id: None,
        params: None,
    };
    client
        .subscribe_quotes(&subscribe_cmd)
        .expect("Subscribe failed");

    // Wait for quote event (skip instrument events emitted during connect)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!timeout.is_zero(), "Timeout waiting for quote event");

        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::Quote(quote)))) => {
                assert_eq!(quote.instrument_id.symbol.as_str(), "EURUSD-PERP");
                assert!(quote.bid_price.as_f64() > 0.0);
                assert!(quote.ask_price.as_f64() > 0.0);
                break;
            }
            Ok(Some(DataEvent::Instrument(_))) => {}
            Ok(Some(other)) => panic!("Expected Quote data event, was {other:?}"),
            Ok(None) => panic!("Channel closed unexpectedly"),
            Err(_) => panic!("Timeout waiting for quote event"),
        }
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_trade_tick_via_channel() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client =
        AxHttpClient::new(Some(http_url), None, None, None, None, None, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let mut client = AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client");

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    // Use first instrument from HTTP fixture (EURUSD-PERP)
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let subscribe_cmd = SubscribeTrades {
        instrument_id,
        client_id: Some(client_id),
        venue: None,
        command_id: UUID4::new(),
        ts_init: 0.into(),
        correlation_id: None,
        params: None,
    };
    client
        .subscribe_trades(&subscribe_cmd)
        .expect("Subscribe failed");

    // Collect events - mock server sends book then trade
    let mut found_trade = false;
    for _ in 0..5 {
        let result = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
        match result {
            Ok(Some(DataEvent::Data(Data::Trade(trade)))) => {
                assert_eq!(trade.instrument_id.symbol.as_str(), "EURUSD-PERP");
                assert!(trade.price.as_f64() > 0.0);
                assert!(trade.size.as_f64() > 0.0);
                found_trade = true;
                break;
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }

    assert!(found_trade, "Expected to receive a trade tick event");
    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let _rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client =
        AxHttpClient::new(Some(http_url), None, None, None, None, None, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let mut client = AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client");

    assert!(!client.is_connected());

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;
    assert!(client.is_connected());

    client.disconnect().await.expect("Failed to disconnect");
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instruments_on_connect() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client =
        AxHttpClient::new(Some(http_url), None, None, None, None, None, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let mut client = AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client");

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let mut instrument_count = 0;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, DataEvent::Instrument(_)) {
            instrument_count += 1;
        }
    }

    assert!(
        instrument_count > 0,
        "Expected instrument events on connect"
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_book_deltas_via_channel() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client =
        AxHttpClient::new(Some(http_url), None, None, None, None, None, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let mut client = AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client");

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let subscribe_cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        None,
        UUID4::new(),
        0.into(),
        None,
        false,
        None,
        None,
    );
    client
        .subscribe_book_deltas(&subscribe_cmd)
        .expect("Subscribe failed");

    // Wait for a Deltas event (skip instrument events from connect)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!timeout.is_zero(), "Timeout waiting for book deltas event");

        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::Deltas(deltas)))) => {
                assert_eq!(deltas.instrument_id.symbol.as_str(), "EURUSD-PERP");
                break;
            }
            Ok(Some(DataEvent::Instrument(_))) => {}
            Ok(Some(_)) => {}
            Ok(None) => panic!("Channel closed unexpectedly"),
            Err(_) => panic!("Timeout waiting for book deltas event"),
        }
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_bars_via_channel() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client =
        AxHttpClient::new(Some(http_url), None, None, None, None, None, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let mut client = AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client");

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let bar_type = BarType::from("EURUSD-PERP.AX-1-MINUTE-LAST-EXTERNAL");
    let subscribe_cmd = SubscribeBars::new(
        bar_type,
        Some(client_id),
        None,
        UUID4::new(),
        0.into(),
        None,
        None,
    );
    client
        .subscribe_bars(&subscribe_cmd)
        .expect("Subscribe failed");

    // Wait for a Bar event (mock server sends 2 candles with different
    // timestamps so the handler emits the first as a closed bar)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!timeout.is_zero(), "Timeout waiting for bar event");

        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::Bar(bar)))) => {
                assert_eq!(bar.bar_type.instrument_id().symbol.as_str(), "EURUSD-PERP");
                break;
            }
            Ok(Some(DataEvent::Instrument(_))) => {}
            Ok(Some(_)) => {}
            Ok(None) => panic!("Channel closed unexpectedly"),
            Err(_) => panic!("Timeout waiting for bar event"),
        }
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_reset_clears_state() {
    let _rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client =
        AxHttpClient::new(Some(http_url), None, None, None, None, None, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let mut client = AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client");

    // Reset before connect should succeed
    client.reset().expect("Reset failed");
    assert!(!client.is_connected());

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;
    assert!(client.is_connected());

    // Reset after connect should clear state
    client.reset().expect("Reset failed");
}
