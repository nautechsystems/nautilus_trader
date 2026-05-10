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
    websocket::{data::client::AxMdWebSocketClient, messages::NautilusDataWsMessage},
};
use nautilus_common::{
    clients::DataClient as DataClientTrait,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent,
        data::{SubscribeQuotes, SubscribeTrades},
    },
};
use nautilus_core::UUID4;
use nautilus_model::{
    data::Data,
    identifiers::{ClientId, InstrumentId},
};
use rstest::rstest;

use crate::common::server::{create_test_instrument, start_test_server, wait_for_connection};

fn setup_data_channel() -> tokio::sync::mpsc::UnboundedReceiver<DataEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(sender);
    receiver
}

#[rstest]
#[tokio::test]
async fn test_handler_parses_l1_to_quote_tick() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    // Cache instrument before connect
    let instrument = create_test_instrument("EURUSD-PERP");
    client.cache_instrument(instrument);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    // Subscribe triggers mock server to send L1 book message
    client
        .subscribe_quotes("EURUSD-PERP")
        .await
        .expect("Subscribe failed");

    // Read from stream and verify quote tick is parsed
    let stream = client.stream();
    tokio::pin!(stream);

    let result = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;

    match result {
        Ok(Some(NautilusDataWsMessage::Data(data_vec))) => {
            assert!(!data_vec.is_empty(), "Expected at least one data item");
            match &data_vec[0] {
                Data::Quote(quote) => {
                    assert_eq!(quote.instrument_id.symbol.as_str(), "EURUSD-PERP");
                    assert!(quote.bid_price.as_f64() > 0.0);
                    assert!(quote.ask_price.as_f64() > 0.0);
                }
                other => panic!("Expected Quote, was {other:?}"),
            }
        }
        Ok(Some(other)) => panic!("Expected Data message, was {other:?}"),
        Ok(None) => panic!("Stream ended unexpectedly"),
        Err(_) => panic!("Timeout waiting for quote tick"),
    }

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_parses_trade_tick() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let instrument = create_test_instrument("EURUSD-PERP");
    client.cache_instrument(instrument);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_trades("EURUSD-PERP")
        .await
        .expect("Subscribe failed");

    let stream = client.stream();
    tokio::pin!(stream);

    // Mock server sends book then trade - skip non-trade messages
    let trade = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.next().await {
                Some(NautilusDataWsMessage::Data(data_vec)) => {
                    for data in data_vec {
                        if let Data::Trade(trade) = data {
                            return trade;
                        }
                    }
                }
                Some(_) => {}
                None => panic!("Stream closed without receiving a trade"),
            }
        }
    })
    .await
    .expect("Timeout waiting for trade tick");

    assert_eq!(trade.instrument_id.symbol.as_str(), "EURUSD-PERP");
    assert!(trade.price.as_f64() > 0.0);
    assert!(trade.size.as_f64() > 0.0);
    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_parses_l2_to_order_book_deltas() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let instrument = create_test_instrument("EURUSD-PERP");
    client.cache_instrument(instrument);

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
        Ok(Some(NautilusDataWsMessage::Deltas(deltas))) => {
            assert_eq!(deltas.instrument_id.symbol.as_str(), "EURUSD-PERP");
        }
        Ok(Some(other)) => panic!("Expected Deltas message, was {other:?}"),
        Ok(None) => panic!("Stream ended unexpectedly"),
        Err(_) => panic!("Timeout waiting for order book deltas"),
    }

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_parses_candle_to_bar() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    let instrument = create_test_instrument("EURUSD-PERP");
    client.cache_instrument(instrument);

    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_candles("EURUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .expect("Subscribe candles failed");

    let stream = client.stream();
    tokio::pin!(stream);

    // Handler only emits bar when candle closes (timestamp changes)
    // Mock server sends two candles, so we should receive one bar
    let bar = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match stream.next().await {
                Some(NautilusDataWsMessage::Bar(bar)) => return bar,
                Some(_) => {}
                None => panic!("Stream closed without receiving a bar"),
            }
        }
    })
    .await
    .expect("Timeout waiting for bar");

    assert_eq!(bar.bar_type.instrument_id().symbol.as_str(), "EURUSD-PERP");
    assert!(bar.open.as_f64() > 0.0);
    assert!(bar.high.as_f64() > 0.0);
    assert!(bar.low.as_f64() > 0.0);
    assert!(bar.close.as_f64() > 0.0);

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_handler_ignores_unknown_symbol() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");
    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    // Don't cache any instrument - messages should be ignored
    client.connect().await.expect("Failed to connect");
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .expect("Subscribe failed");

    let stream = client.stream();
    tokio::pin!(stream);

    // Should timeout because messages are ignored (no instrument cached)
    let result = tokio::time::timeout(Duration::from_millis(500), stream.next()).await;

    assert!(
        result.is_err(),
        "Expected timeout when instrument not cached"
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
