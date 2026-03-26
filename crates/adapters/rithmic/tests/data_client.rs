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

//! Integration tests for the public `RithmicDataClient` API.
//!
//! These tests cover the first crate-level smoke harness for the Rithmic shadow port.
//! They intentionally exercise the public client and gateway API without live plants so
//! the `tests/` tree exists before the deeper mock-transport layer lands.

mod common;

use std::sync::Arc;

use nautilus_rithmic::common::enums::ConnectionState;
use nautilus_rithmic::data::MarketDataEvent;
use tokio::time::{Duration, timeout};

use crate::common::{
    MockTickerPlant, assert_connection_error, test_data_client, test_ticker_only_gateway_config,
};

#[tokio::test]
async fn subscribe_quotes_requires_connected_gateway_without_mutating_tracking() {
    let client = test_data_client();

    let err = client.subscribe_quotes("ESM6", "CME").await.unwrap_err();

    assert_connection_error(err, "Not connected");
    assert_eq!(client.connection_state(), ConnectionState::Disconnected);
    assert!(!client.is_connected());
    assert_eq!(client.subscription_count(), 0);
    assert!(client.subscriptions().is_empty());
    assert!(!client.is_subscribed_quotes("ESM6", "CME"));
    assert!(!client.is_subscribed_trades("ESM6", "CME"));
}

#[tokio::test]
async fn subscribe_trades_requires_connected_gateway_without_mutating_tracking() {
    let client = test_data_client();

    let err = client.subscribe_trades("ESM6", "CME").await.unwrap_err();

    assert_connection_error(err, "Not connected");
    assert_eq!(client.subscription_count(), 0);
    assert!(!client.is_subscribed_quotes("ESM6", "CME"));
    assert!(!client.is_subscribed_trades("ESM6", "CME"));
}

#[tokio::test]
async fn subscribe_combined_requires_connected_gateway_without_mutating_tracking() {
    let client = test_data_client();

    let err = client.subscribe("ESM6", "CME").await.unwrap_err();

    assert_connection_error(err, "Not connected");
    assert_eq!(client.subscription_count(), 0);
    assert!(client.subscriptions().is_empty());
}

#[tokio::test]
async fn subscribe_quotes_connected_path_emits_authenticated_quote_and_trade() {
    let server = MockTickerPlant::start().await;
    let config = test_ticker_only_gateway_config(&server.url);
    let mut gateway = nautilus_rithmic::RithmicGateway::new(config);
    let mut rx = gateway.take_market_data_receiver().unwrap();

    gateway.connect().await.unwrap();
    assert!(gateway.is_connected());

    expect_authenticated(&mut rx).await;

    let gateway = Arc::new(gateway);
    let client = nautilus_rithmic::RithmicDataClient::new(Arc::clone(&gateway));

    client.subscribe_quotes("ESM6", "CME").await.unwrap();

    assert!(client.is_subscribed_quotes("ESM6", "CME"));
    assert!(!client.is_subscribed_trades("ESM6", "CME"));
    assert_eq!(client.subscription_count(), 1);
    assert_eq!(server.subscribe_requests(), 1);

    let mut quote_seen = false;
    let mut trade_seen = false;

    for _ in 0..4 {
        match next_market_data_event(&mut rx).await {
            MarketDataEvent::Quote(quote) => {
                assert_eq!(quote.symbol, "ESM6");
                assert_eq!(quote.exchange, "CME");
                assert_eq!(quote.bid_price, 4500.25);
                assert_eq!(quote.ask_price, 4500.50);
                assert_eq!(quote.bid_size, 10.0);
                assert_eq!(quote.ask_size, 12.0);
                quote_seen = true;
            }
            MarketDataEvent::Trade(trade) => {
                assert_eq!(trade.symbol, "ESM6");
                assert_eq!(trade.exchange, "CME");
                assert_eq!(trade.price, 4500.50);
                assert_eq!(trade.size, 3.0);
                assert_eq!(trade.aggressor_side, "BUY");
                assert_eq!(trade.trade_id, "trade-1");
                trade_seen = true;
            }
            MarketDataEvent::ConnectionState(_) | MarketDataEvent::Error(_) => {}
            other => panic!("unexpected market data event {other:?}"),
        }

        if quote_seen && trade_seen {
            break;
        }
    }

    assert!(quote_seen, "expected quote event");
    assert!(trade_seen, "expected trade event");

    drop(client);
    let mut gateway = Arc::try_unwrap(gateway).unwrap();
    gateway.disconnect().await.unwrap();
    server.wait().await;
}

#[tokio::test]
async fn subscribe_quotes_then_trades_uses_single_connected_market_data_subscription() {
    let server = MockTickerPlant::start().await;
    let config = test_ticker_only_gateway_config(&server.url);
    let mut gateway = nautilus_rithmic::RithmicGateway::new(config);
    let mut rx = gateway.take_market_data_receiver().unwrap();

    gateway.connect().await.unwrap();
    expect_authenticated(&mut rx).await;

    let gateway = Arc::new(gateway);
    let client = nautilus_rithmic::RithmicDataClient::new(Arc::clone(&gateway));

    client.subscribe_quotes("ESM6", "CME").await.unwrap();
    client.subscribe_trades("ESM6", "CME").await.unwrap();

    assert!(client.is_subscribed_quotes("ESM6", "CME"));
    assert!(client.is_subscribed_trades("ESM6", "CME"));
    assert_eq!(client.subscription_count(), 1);
    assert_eq!(server.subscribe_requests(), 1);

    drop(client);
    let mut gateway = Arc::try_unwrap(gateway).unwrap();
    gateway.disconnect().await.unwrap();
    server.wait().await;
}

async fn next_market_data_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<MarketDataEvent>,
) -> MarketDataEvent {
    timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for market data event")
        .expect("market data channel closed unexpectedly")
}

async fn expect_authenticated(rx: &mut tokio::sync::mpsc::UnboundedReceiver<MarketDataEvent>) {
    for _ in 0..4 {
        match next_market_data_event(rx).await {
            MarketDataEvent::Authenticated => return,
            MarketDataEvent::ConnectionState(ConnectionState::Connecting)
            | MarketDataEvent::ConnectionState(ConnectionState::Connected) => {}
            other => panic!("expected authenticated event, got {other:?}"),
        }
    }

    panic!("did not observe authenticated event");
}
