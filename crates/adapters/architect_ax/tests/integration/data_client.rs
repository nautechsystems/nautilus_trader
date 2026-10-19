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

use std::{net::SocketAddr, sync::atomic::Ordering, time::Duration};

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
    live::runner::{replace_system_event_sender, set_data_event_sender},
    messages::{
        DataEvent, DataResponse, SystemEvent,
        data::{
            RequestInstrument, SubscribeBars, SubscribeBookDeltas, SubscribeQuotes,
            SubscribeTrades, UnsubscribeBookDeltas,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::UUID4;
use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas},
    enums::{BookAction, BookType, RecordFlag},
    identifiers::{ClientId, InstrumentId},
    instruments::Instrument,
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use serde_json::Value;
use ustr::Ustr;

use crate::common::server::{TestServerState, start_test_server, wait_for_connection};

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
    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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
            assert_eq!(book.s, "EURUSD-PERP");
        }
        Ok(Some(other)) => panic!("Expected MdMessage::BookL1, was {other:?}"),
        Ok(None) => panic!("Stream ended unexpectedly"),
        Err(_) => panic!("Timeout waiting for L1 message"),
    }

    client.close().await.expect("close WebSocket client");
}

#[rstest]
#[tokio::test]
async fn test_handler_emits_trade_md_message() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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

    assert_eq!(trade.s, "EURUSD-PERP");
    client.close().await.expect("close WebSocket client");
}

#[rstest]
#[tokio::test]
async fn test_handler_emits_l2_md_message() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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
            assert_eq!(book.s, "EURUSD-PERP");
        }
        Ok(Some(other)) => panic!("Expected MdMessage::BookL2, was {other:?}"),
        Ok(None) => panic!("Stream ended unexpectedly"),
        Err(_) => panic!("Timeout waiting for order book message"),
    }

    client.close().await.expect("close WebSocket client");
}

#[rstest]
#[tokio::test]
async fn test_handler_emits_candle_md_message() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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

    assert_eq!(candle.symbol, "EURUSD-PERP");

    client.close().await.expect("close WebSocket client");
}

#[rstest]
#[tokio::test]
async fn test_handler_forwards_raw_message_even_when_instrument_missing() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .expect("Subscribe failed");

    let stream = client.stream();
    tokio::pin!(stream);

    // The handler forwards raw venue messages. Downstream consumers filter by
    // symbol, so an MD message should arrive even if the caller had not cached
    // the instrument yet. Verify the first message is the expected L1 book.
    let msg = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("timeout waiting for WS message")
        .expect("stream ended");

    match msg {
        AxDataWsMessage::MdMessage(AxMdMessage::BookL1(book)) => {
            assert_eq!(book.s, "EURUSD-PERP");
        }
        other => panic!("expected BookL1, was {other:?}"),
    }

    client.close().await.expect("close WebSocket client");
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_quote_tick_via_channel() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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
        .subscribe_quotes(subscribe_cmd)
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

    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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
        .subscribe_trades(subscribe_cmd)
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
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    let config = AxDataClientConfig::default();
    let client_id = ClientId::from("AX-TEST");
    let registry = SocketReconnectRegistry::default();
    let mut client = registry
        .scope(|| AxDataClient::new(client_id, config, http_client, ws_client))
        .expect("Failed to create data client");

    assert!(!client.is_connected());

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;
    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("architect-ax-data-streams");
    let handle = registry.handle(client_id, endpoint).unwrap();

    assert!(client.is_connected());
    assert_eq!(change.client_id, client_id);
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);

    client.disconnect().await.expect("Failed to disconnect");
    assert!(!client.is_connected());
    assert!(registry.handle(client_id, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instruments_on_connect() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");

    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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

    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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
        .subscribe_book_deltas(subscribe_cmd)
        .expect("Subscribe failed");

    // Wait for a Deltas event (skip instrument events from connect)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!timeout.is_zero(), "Timeout waiting for book deltas event");

        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::BookDeltas(deltas)))) => {
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

    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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
        .subscribe_bars(subscribe_cmd)
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

    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

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

#[rstest]
#[tokio::test]
async fn test_data_client_request_instrument_emits_response() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let client_id = ClientId::from("AX-TEST");
    let mut client = create_data_client(addr, client_id, AxDataClientConfig::default());

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            request_id,
            0.into(),
            None,
        ))
        .expect("Instrument request failed");

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("Timeout waiting for instrument response")
        .expect("Channel closed unexpectedly");
    let DataEvent::Response(DataResponse::Instrument(response)) = event else {
        panic!("Expected instrument response, was {event:?}");
    };

    assert_eq!(response.correlation_id, request_id);
    assert_eq!(response.client_id, client_id);
    assert_eq!(response.instrument_id, instrument_id);
    assert_eq!(response.data.id(), instrument_id);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_disconnect_aborts_instrument_request() {
    let mut rx = setup_data_channel();

    let (addr, state) = start_test_server().await.unwrap();
    let client_id = ClientId::from("AX-TEST");
    let mut client = create_data_client(addr, client_id, AxDataClientConfig::default());

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    while rx.try_recv().is_ok() {}

    state
        .instrument_response_delay
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let request_id = UUID4::new();
    let entered = state.instrument_response_entered.notified();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            request_id,
            0.into(),
            None,
        ))
        .expect("Instrument request failed");
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .expect("Instrument request did not enter the server");

    let dropped = state.instrument_response_dropped.notified();
    client.disconnect().await.expect("Failed to disconnect");
    let request_aborted = tokio::time::timeout(Duration::from_secs(5), dropped)
        .await
        .is_ok();
    state.instrument_response_release.notify_one();

    assert!(
        request_aborted,
        "Instrument request remained active after disconnect"
    );

    while let Ok(event) = rx.try_recv() {
        if let DataEvent::Response(DataResponse::Instrument(response)) = event {
            assert_ne!(
                response.correlation_id, request_id,
                "Disconnected request emitted a late response"
            );
        }
    }

    state
        .instrument_response_delay
        .store(false, std::sync::atomic::Ordering::Relaxed);
    client.connect().await.expect("Failed to reconnect");
    wait_for_connection(&state).await;

    while rx.try_recv().is_ok() {}

    let reconnect_request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            reconnect_request_id,
            0.into(),
            None,
        ))
        .expect("Instrument request after reconnect failed");
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("Timeout waiting for instrument response after reconnect")
        .expect("Channel closed unexpectedly");
    let DataEvent::Response(DataResponse::Instrument(response)) = event else {
        panic!("Expected instrument response after reconnect, was {event:?}");
    };

    assert_eq!(response.correlation_id, reconnect_request_id);
    assert_eq!(response.client_id, client_id);
    assert_eq!(response.instrument_id, instrument_id);
    assert_eq!(response.data.id(), instrument_id);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_recovers_missing_initial_book_snapshot() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    state.withhold_book.store(true, Ordering::Relaxed);
    let client_id = ClientId::from("AX-TEST");
    let mut client = create_data_client(addr, client_id, book_config(1));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    wait_for_requests(&state, "unsubscribe", 1, Duration::from_secs(10)).await;

    state.withhold_book.store(false, Ordering::Relaxed);
    let recovered = wait_for_book_deltas(&mut rx).await;

    let subscribes = count_requests(&state, "subscribe").await;
    let unsubscribes = count_requests(&state, "unsubscribe").await;
    assert_snapshot(&recovered, instrument_id);
    assert_eq!(
        subscribes,
        unsubscribes + 1,
        "each replacement follows the initial subscribe with one unsubscribe",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_recovers_book_after_invalid_frame() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    state.invalid_book.store(true, Ordering::Relaxed);
    let client_id = ClientId::from("AX-TEST");

    // Far beyond the test's wait, so only invalid frames can start recovery and fail its attempts
    let mut client = create_data_client(addr, client_id, book_config(30));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    wait_for_requests(&state, "unsubscribe", 2, Duration::from_secs(8)).await;

    state.invalid_book.store(false, Ordering::Relaxed);
    let recovered = wait_for_book_deltas(&mut rx).await;

    let subscribes = count_requests(&state, "subscribe").await;
    let unsubscribes = count_requests(&state, "unsubscribe").await;
    assert_snapshot(&recovered, instrument_id);
    assert_eq!(subscribes, unsubscribes + 1);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_recovers_book_snapshot_missing_after_reconnect() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    let client_id = ClientId::from("AX-TEST");
    let registry = SocketReconnectRegistry::default();
    let mut client = registry.scope(|| create_data_client(addr, client_id, book_config(1)));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    wait_for_book_deltas(&mut rx).await;

    // The reconnect replays the subscription, which now delivers no snapshot
    state.withhold_book.store(true, Ordering::Relaxed);
    let handle = registry
        .handle(client_id, Ustr::from("architect-ax-data-streams"))
        .unwrap();
    let reconnect = handle.request_reconnect();
    wait_for_requests(&state, "unsubscribe", 1, Duration::from_secs(15)).await;

    state.withhold_book.store(false, Ordering::Relaxed);
    let recovered = wait_for_book_deltas(&mut rx).await;

    let subscribes = count_requests(&state, "subscribe").await;
    let unsubscribes = count_requests(&state, "unsubscribe").await;
    assert_eq!(reconnect, SocketReconnectRequestOutcome::Accepted);
    assert_snapshot(&recovered, instrument_id);
    assert_eq!(
        subscribes,
        unsubscribes + 2,
        "the initial subscribe and the reconnect replay precede each replacement",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_recovers_book_snapshot_missing_after_client_reconnect() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    let client_id = ClientId::from("AX-TEST");
    let mut client = create_data_client(addr, client_id, book_config(1));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    wait_for_book_deltas(&mut rx).await;

    // The client keeps its book across the reconnect, and the replay delivers no snapshot
    state.withhold_book.store(true, Ordering::Relaxed);
    client.disconnect().await.expect("Failed to disconnect");
    client.connect().await.expect("Failed to reconnect");
    wait_for_requests(&state, "unsubscribe", 1, Duration::from_secs(15)).await;

    state.withhold_book.store(false, Ordering::Relaxed);
    let recovered = wait_for_book_deltas(&mut rx).await;

    let subscribes = count_requests(&state, "subscribe").await;
    let unsubscribes = count_requests(&state, "unsubscribe").await;
    assert_snapshot(&recovered, instrument_id);
    assert_eq!(
        subscribes,
        unsubscribes + 2,
        "the initial subscribe and the connect replay precede each replacement",
    );

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_reconnect_replays_book_after_recovery() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    state.withhold_book.store(true, Ordering::Relaxed);
    let client_id = ClientId::from("AX-TEST");
    let registry = SocketReconnectRegistry::default();
    let mut client = registry.scope(|| create_data_client(addr, client_id, book_config(1)));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    wait_for_requests(&state, "unsubscribe", 1, Duration::from_secs(10)).await;

    state.withhold_book.store(false, Ordering::Relaxed);
    wait_for_book_deltas(&mut rx).await;
    let subscribes = count_requests(&state, "subscribe").await;
    let unsubscribes = count_requests(&state, "unsubscribe").await;

    // A replacement keeps the subscription in replay, so the reconnect alone restores the book
    let handle = registry
        .handle(client_id, Ustr::from("architect-ax-data-streams"))
        .unwrap();
    let reconnect = handle.request_reconnect();
    let replayed = wait_for_book_deltas(&mut rx).await;

    assert_eq!(reconnect, SocketReconnectRequestOutcome::Accepted);
    assert_snapshot(&replayed, instrument_id);
    assert_eq!(count_requests(&state, "subscribe").await, subscribes + 1);
    assert_eq!(count_requests(&state, "unsubscribe").await, unsubscribes);

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_suppresses_book_frames_after_unsubscribe() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    let client_id = ClientId::from("AX-TEST");
    let mut client = create_data_client(addr, client_id, book_config(30));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    wait_for_book_deltas(&mut rx).await;

    // The venue answers the unsubscribe with a frame it sent before processing it
    state.trailing_book.store(true, Ordering::Relaxed);
    client
        .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
            instrument_id,
            Some(client_id),
            None,
            UUID4::new(),
            0.into(),
            None,
            None,
        ))
        .expect("Unsubscribe failed");
    wait_for_requests(&state, "unsubscribe", 1, Duration::from_secs(5)).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut late_deltas = 0;

    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if matches!(event, DataEvent::Data(Data::BookDeltas(_))) {
            late_deltas += 1;
        }
    }

    assert_eq!(late_deltas, 0);

    client.disconnect().await.expect("Failed to disconnect");
}

// Each resubscribe races the unsubscribe before it across worker threads
#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_data_client_applies_book_resubscribe_after_unsubscribe() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    let client_id = ClientId::from("AX-TEST");
    let mut client = create_data_client(addr, client_id, book_config(30));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    wait_for_book_deltas(&mut rx).await;

    for _ in 0..10 {
        state.messages_received.lock().await.clear();
        client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                instrument_id,
                Some(client_id),
                None,
                UUID4::new(),
                0.into(),
                None,
                None,
            ))
            .expect("Unsubscribe failed");
        client
            .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
            .expect("Resubscribe failed");
        let resubscribed = wait_for_book_deltas(&mut rx).await;

        let requests: Vec<String> = state
            .get_messages()
            .await
            .iter()
            .filter_map(|message| message.get("type").and_then(Value::as_str))
            .map(ToString::to_string)
            .collect();

        assert_snapshot(&resubscribed, instrument_id);
        assert_eq!(requests, ["unsubscribe", "subscribe"]);
        assert_eq!(
            *state.subscriptions.lock().await,
            ["EURUSD-PERP:LEVEL_2".to_string()]
        );
    }

    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_data_client_book_resubscribe_starts_from_fresh_snapshot() {
    let mut rx = setup_data_channel();
    let (addr, state) = start_test_server().await.unwrap();
    let client_id = ClientId::from("AX-TEST");
    let mut client = create_data_client(addr, client_id, book_config(30));
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Subscribe failed");
    let initial = wait_for_book_deltas(&mut rx).await;
    client
        .subscribe_book_deltas(book_deltas_subscription(instrument_id, client_id))
        .expect("Resubscribe failed");
    let replaced = wait_for_book_deltas(&mut rx).await;

    assert_snapshot(&initial, instrument_id);
    assert_snapshot(&replaced, instrument_id);
    assert_eq!(count_requests(&state, "subscribe").await, 2);
    assert_eq!(count_requests(&state, "unsubscribe").await, 1);

    client.disconnect().await.expect("Failed to disconnect");
}

fn create_data_client(
    addr: SocketAddr,
    client_id: ClientId,
    config: AxDataClientConfig,
) -> AxDataClient {
    let http_url = format!("http://{addr}");
    let ws_url = format!("ws://{addr}/md/ws");
    let http_client = AxHttpClient::new(Some(http_url), None, 60, 3, 1000, 10_000, None).unwrap();
    let ws_client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    AxDataClient::new(client_id, config, http_client, ws_client)
        .expect("Failed to create data client")
}

fn book_config(book_snapshot_timeout_secs: u64) -> AxDataClientConfig {
    AxDataClientConfig {
        book_snapshot_timeout_secs,
        ..AxDataClientConfig::default()
    }
}

fn book_deltas_subscription(
    instrument_id: InstrumentId,
    client_id: ClientId,
) -> SubscribeBookDeltas {
    SubscribeBookDeltas::new(
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
    )
}

async fn count_requests(state: &TestServerState, request_type: &str) -> usize {
    state
        .get_messages()
        .await
        .iter()
        .filter(|message| message.get("type").and_then(Value::as_str) == Some(request_type))
        .filter(|message| message.get("symbol").and_then(Value::as_str) == Some("EURUSD-PERP"))
        .count()
}

async fn wait_for_requests(
    state: &TestServerState,
    request_type: &str,
    count: usize,
    timeout: Duration,
) {
    wait_until_async(
        || async { count_requests(state, request_type).await >= count },
        timeout,
    )
    .await;
}

async fn wait_for_book_deltas(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> OrderBookDeltas {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("Timeout waiting for book deltas")
            .expect("Data event channel closed");

        if let DataEvent::Data(Data::BookDeltas(deltas)) = event {
            return *deltas;
        }
    }
}

// Asserts the `ws_md_book_l2.json` snapshot as one event group closed by `F_LAST` once
fn assert_snapshot(deltas: &OrderBookDeltas, instrument_id: InstrumentId) {
    let snapshot = RecordFlag::F_SNAPSHOT as u8;
    let level = RecordFlag::F_MBP as u8 | snapshot;
    let last = RecordFlag::F_LAST as u8;
    let flags: Vec<u8> = deltas.deltas.iter().map(|delta| delta.flags).collect();
    let actions: Vec<BookAction> = deltas.deltas.iter().map(|delta| delta.action).collect();

    assert_eq!(deltas.instrument_id, instrument_id);
    assert_eq!(
        actions,
        [vec![BookAction::Clear], vec![BookAction::Add; 6],].concat()
    );
    assert_eq!(
        flags,
        vec![snapshot, level, level, level, level, level, level | last]
    );
}
