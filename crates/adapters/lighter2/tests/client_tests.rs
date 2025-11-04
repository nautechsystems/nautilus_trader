// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Tests for HTTP and WebSocket clients.

use nautilus_lighter2::{
    http::LighterHttpClient,
    websocket::LighterWebSocketClient,
    common::enums::LighterWsChannel,
};

#[test]
fn test_http_client_creation() {
    let client = LighterHttpClient::new(None, None, false, None);
    assert_eq!(client.instruments().len(), 0);
}

#[test]
fn test_http_client_with_credentials() {
    use nautilus_lighter2::common::credential::LighterCredentials;

    let creds = LighterCredentials::new(
        "test_key".to_string(),
        "test_eth".to_string(),
        2,
        1,
    ).unwrap();

    let client = LighterHttpClient::new(None, None, false, Some(creds));
    assert_eq!(client.instruments().len(), 0);
}

#[test]
fn test_ws_client_creation() {
    let client = LighterWebSocketClient::new(None, None, false, None);
    let debug_str = format!("{:?}", client);
    assert!(debug_str.contains("LighterWebSocketClient"));
}

#[tokio::test]
async fn test_ws_subscription_management() {
    let client = LighterWebSocketClient::new(None, None, false, None);
    let channel = LighterWsChannel::OrderBook { market_id: 0 };

    assert_eq!(client.subscription_count().await, 0);
    assert!(!client.is_subscribed(&channel).await);

    client.subscribe(channel.clone()).await.unwrap();
    assert_eq!(client.subscription_count().await, 1);
    assert!(client.is_subscribed(&channel).await);

    client.unsubscribe(&channel).await.unwrap();
    assert_eq!(client.subscription_count().await, 0);
    assert!(!client.is_subscribed(&channel).await);
}

#[tokio::test]
async fn test_ws_multiple_subscriptions() {
    let client = LighterWebSocketClient::new(None, None, false, None);

    let channel1 = LighterWsChannel::OrderBook { market_id: 0 };
    let channel2 = LighterWsChannel::Trades { market_id: 0 };
    let channel3 = LighterWsChannel::Account { account_id: 1 };

    client.subscribe(channel1).await.unwrap();
    client.subscribe(channel2).await.unwrap();
    client.subscribe(channel3).await.unwrap();

    assert_eq!(client.subscription_count().await, 3);
}

#[test]
fn test_ws_channel_display() {
    let channel1 = LighterWsChannel::OrderBook { market_id: 5 };
    assert_eq!(channel1.to_string(), "orderbook:5");

    let channel2 = LighterWsChannel::Trades { market_id: 10 };
    assert_eq!(channel2.to_string(), "trades:10");

    let channel3 = LighterWsChannel::Account { account_id: 1 };
    assert_eq!(channel3.to_string(), "account:1");

    let channel4 = LighterWsChannel::Orders { account_id: 2 };
    assert_eq!(channel4.to_string(), "orders:2");
}
