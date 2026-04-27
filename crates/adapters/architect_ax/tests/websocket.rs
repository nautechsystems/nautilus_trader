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

//! Integration tests for Ax WebSocket clients using a mock server.

mod common;

use std::{sync::atomic::Ordering, time::Duration};

use nautilus_architect_ax::{
    common::enums::{AxCandleWidth, AxMarketDataLevel},
    websocket::{data::AxMdWebSocketClient, orders::AxOrdersWebSocketClient},
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, StrategyId, TraderId, VenueOrderId},
    instruments::Instrument,
    types::{Price, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use ustr::Ustr;

use crate::common::server::{create_test_instrument, start_test_server, wait_for_connection};

#[rstest]
#[tokio::test]
async fn test_md_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_client_url_accessor() {
    let ws_url = "ws://localhost:9999/md/ws".to_string();
    let client = AxMdWebSocketClient::new(
        ws_url.clone(),
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    assert_eq!(client.url(), ws_url);
}

#[rstest]
#[tokio::test]
async fn test_md_client_not_active_before_connect() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    assert!(!client.is_active());
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_md_connection_failure_to_invalid_url() {
    let mut client = AxMdWebSocketClient::new(
        "ws://127.0.0.1:9999/invalid".to_string(),
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    let result = client.connect().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_md_close_sets_closed_flag() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    assert!(!client.is_closed());

    client.close().await;

    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_md_disconnect_without_close() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client.disconnect().await;

    wait_until_async(|| async { !client.is_active() }, Duration::from_secs(5)).await;

    // Disconnect doesn't set closed flag
    assert!(!client.is_closed());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_l1() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("EURUSD-PERP") && s.contains("LEVEL_1"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_l2() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("EURUSD-PERP") && s.contains("LEVEL_2"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_l3() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level3)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("EURUSD-PERP") && s.contains("LEVEL_3"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_multiple_symbols() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();
    client
        .subscribe_book_deltas("ETHUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .unwrap();
    client
        .subscribe_book_deltas("GBPUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(subs.iter().any(|s| s.contains("EURUSD-PERP")));
    assert!(subs.iter().any(|s| s.contains("ETHUSD-PERP")));
    assert!(subs.iter().any(|s| s.contains("GBPUSD-PERP")));

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_unsubscribe() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_book_deltas("EURUSD-PERP").await.unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.subscriptions.lock().await.is_empty());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_candles() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_candles("EURUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("EURUSD-PERP") && s.contains("candle"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_unsubscribe_candles() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_candles("EURUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe_candles("EURUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.subscriptions.lock().await.is_empty());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscription_count_starts_at_zero() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    assert_eq!(client.subscription_count(), 0);
}

#[rstest]
#[tokio::test]
async fn test_md_ping_pong() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        1, // 1 second heartbeat,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    wait_until_async(
        || async { state.ping_count.load(Ordering::Relaxed) > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.ping_count.load(Ordering::Relaxed) > 0);

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_server_disconnect_handling() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    wait_until_async(
        || async { *state.connection_count.lock().await == 0 },
        Duration::from_secs(5),
    )
    .await;

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_reconnection_after_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url.clone(),
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    let initial_count = *state.connection_count.lock().await;
    assert_eq!(initial_count, 1);

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    wait_until_async(
        || async { *state.connection_count.lock().await == 0 },
        Duration::from_secs(5),
    )
    .await;

    state.reset().await;

    let mut client2 = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client2.connect().await.unwrap();
    wait_for_connection(&state).await;

    assert!(client2.is_active());

    client.close().await;
    client2.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_rapid_subscribe_unsubscribe() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    for _ in 0..5 {
        client
            .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
            .await
            .unwrap();
        client.unsubscribe_book_deltas("EURUSD-PERP").await.unwrap();
    }

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_quotes_then_book_l2_resubscribes() {
    // Subscribing quotes yields effective level L1. A subsequent book_deltas at L2
    // changes the effective level; update_data_subscription should unsubscribe L1
    // then subscribe L2, ending with exactly one L2 subscription on the server.
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );
    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_quotes("EURUSD-PERP")
        .await
        .expect("Subscribe quotes failed");

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("LEVEL_1"))
        },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .expect("Subscribe book L2 failed");

    wait_until_async(
        || async {
            let subs = state.subscriptions.lock().await;
            subs.iter().any(|s| s.contains("LEVEL_2"))
                && !subs.iter().any(|s| s.contains("LEVEL_1"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(
        subs.len(),
        1,
        "expected 1 live subscription, found {subs:?}"
    );
    assert!(subs[0].contains("LEVEL_2"));

    // Verify the transition: inbound messages include L1 subscribe, unsubscribe,
    // then L2 subscribe.
    let messages = state.get_messages().await;
    let subscribe_levels: Vec<String> = messages
        .iter()
        .filter(|m| m.get("type").and_then(|v| v.as_str()) == Some("subscribe"))
        .filter_map(|m| m.get("level").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    assert!(
        subscribe_levels.iter().any(|l| l == "LEVEL_1"),
        "expected an L1 subscribe message, levels={subscribe_levels:?}",
    );
    assert!(
        subscribe_levels.iter().any(|l| l == "LEVEL_2"),
        "expected an L2 subscribe message, levels={subscribe_levels:?}",
    );
    let unsubscribe_count = messages
        .iter()
        .filter(|m| m.get("type").and_then(|v| v.as_str()) == Some("unsubscribe"))
        .count();
    assert!(
        unsubscribe_count >= 1,
        "expected at least one unsubscribe during level change",
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_same_level_is_idempotent() {
    // Subscribing quotes and then mark_prices leaves effective level at L1;
    // update_data_subscription should short-circuit without additional traffic.
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );
    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_quotes("EURUSD-PERP")
        .await
        .expect("Subscribe quotes failed");

    wait_until_async(
        || async { !state.subscription_events().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let initial_event_count = state.subscription_events().await.len();
    client
        .subscribe_mark_prices("EURUSD-PERP")
        .await
        .expect("Subscribe mark prices failed");

    // Give the handler a moment to (not) send anything
    tokio::time::sleep(Duration::from_millis(100)).await;
    let events_after = state.subscription_events().await;
    assert_eq!(
        events_after.len(),
        initial_event_count,
        "no new subscribe traffic expected, events_after={events_after:?}",
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_unsubscribe_last_data_type_removes_server_subscription() {
    // Some(L1) -> None transition when the last data type is unsubscribed
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );
    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_quotes("EURUSD-PERP")
        .await
        .expect("Subscribe quotes failed");
    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe_quotes("EURUSD-PERP")
        .await
        .expect("Unsubscribe quotes failed");

    wait_until_async(
        || async { state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.subscriptions.lock().await.is_empty());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_same_symbol_different_levels() {
    // Architect allows only one subscription per symbol - the second subscription
    // at a different level should be skipped (deduplication)
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();
    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .unwrap();

    // Only one subscription should be sent (L1), L2 should be skipped
    wait_until_async(
        || async { state.subscriptions.lock().await.len() == 1 },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(
        subs.len(),
        1,
        "Expected 1 subscription, found {}",
        subs.len()
    );
    assert!(subs.iter().any(|s| s.contains("LEVEL_1")));

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");

    let mut client = AxOrdersWebSocketClient::new(
        ws_url,
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    client.connect("test_bearer_token").await.unwrap();
    wait_for_connection(&state).await;

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_client_url_accessor() {
    let ws_url = "ws://localhost:9999/orders/ws".to_string();
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");
    let client = AxOrdersWebSocketClient::new(
        ws_url.clone(),
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    assert_eq!(client.url(), ws_url);
}

#[rstest]
#[tokio::test]
async fn test_orders_client_account_id_accessor() {
    let ws_url = "ws://localhost:9999/orders/ws".to_string();
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");
    let client = AxOrdersWebSocketClient::new(
        ws_url,
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    assert_eq!(client.account_id(), account_id);
}

#[rstest]
#[tokio::test]
async fn test_orders_client_not_active_before_connect() {
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    assert!(!client.is_active());
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_orders_connection_failure_to_invalid_url() {
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");
    let mut client = AxOrdersWebSocketClient::new(
        "ws://127.0.0.1:9999/invalid".to_string(),
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    let result = client.connect("test_token").await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_orders_close_sets_closed_flag() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");

    let mut client = AxOrdersWebSocketClient::new(
        ws_url,
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    assert!(!client.is_closed());

    client.close().await;

    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_orders_submit_order() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");

    let mut client = AxOrdersWebSocketClient::new(
        ws_url,
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    // Cache instrument before submitting order
    let instrument = create_test_instrument("EURUSD-PERP");
    client.cache_instrument(instrument.clone());

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    let request_id = client
        .submit_order(
            trader_id,
            StrategyId::from("TEST-STRATEGY"),
            instrument.id(),
            ClientOrderId::from("TEST-001"),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            false,
        )
        .await
        .unwrap();

    assert!(request_id > 0);

    wait_until_async(
        || async { !state.get_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.get_messages().await;
    let place = messages
        .iter()
        .find(|m| m.get("t").and_then(|v| v.as_str()) == Some("p"))
        .expect("expected a place-order message");
    assert_eq!(place.get("s").and_then(|v| v.as_str()), Some("EURUSD-PERP"));
    assert_eq!(place.get("d").and_then(|v| v.as_str()), Some("B"));
    assert_eq!(place.get("q").and_then(|v| v.as_i64()), Some(100));
    assert_eq!(place.get("p").and_then(|v| v.as_str()), Some("50000.00"));
    assert_eq!(place.get("tif").and_then(|v| v.as_str()), Some("GTC"));

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_cancel_order_rejects_without_venue_order_id() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");

    let mut client = AxOrdersWebSocketClient::new(
        ws_url,
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    let client_order_id = ClientOrderId::from("O-123");
    let result = client.cancel_order(client_order_id, None).await;
    assert!(result.is_err());

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(state.get_messages().await.is_empty());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_cancel_order_with_venue_order_id() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");

    let mut client = AxOrdersWebSocketClient::new(
        ws_url,
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    let client_order_id = ClientOrderId::from("O-123");
    let venue_order_id = VenueOrderId::new("OID-123");
    let request_id = client
        .cancel_order(client_order_id, Some(venue_order_id))
        .await
        .unwrap();

    assert!(request_id > 0);

    wait_until_async(
        || async { !state.get_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    // AxWsCancelOrder serializes t="x" (CancelOrder request type)
    let messages = state.get_messages().await;
    let cancel = messages
        .iter()
        .find(|m| m.get("t").and_then(|v| v.as_str()) == Some("x"))
        .expect("expected a cancel-order message");
    assert_eq!(cancel.get("oid").and_then(|v| v.as_str()), Some("OID-123"));

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_get_open_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");

    let mut client = AxOrdersWebSocketClient::new(
        ws_url,
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    let request_id = client.get_open_orders().await.unwrap();

    assert!(request_id > 0);

    wait_until_async(
        || async { !state.get_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.get_messages().await;
    let request = messages
        .iter()
        .find(|m| m.get("t").and_then(|v| v.as_str()) == Some("o"))
        .expect("expected a get-open-orders message");
    assert_eq!(
        request.get("rid").and_then(|v| v.as_i64()),
        Some(request_id)
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_cache_instrument() {
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    let instrument = create_test_instrument("EURUSD-PERP");
    client.cache_instrument(instrument);

    let cached = client.get_cached_instrument(&Ustr::from("EURUSD-PERP"));
    assert!(cached.is_some());
}

#[rstest]
#[tokio::test]
async fn test_orders_get_cached_instrument_returns_none_for_unknown() {
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    let cached = client.get_cached_instrument(&Ustr::from("UNKNOWN-SYMBOL"));
    assert!(cached.is_none());
}

#[rstest]
#[tokio::test]
async fn test_md_subscription_events_tracking() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscription_events().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(topic, success)| topic.contains("EURUSD-PERP") && *success)
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscription_failure_tracking() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    state
        .set_subscription_failures(vec!["FAIL-SYMBOL:LEVEL_1".to_string()])
        .await;

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_book_deltas("FAIL-SYMBOL", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscription_events().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(topic, success)| topic.contains("FAIL-SYMBOL") && !*success)
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_multiple_md_clients() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client1 = AxMdWebSocketClient::new(
        ws_url.clone(),
        "token1".to_string(),
        30,
        TransportBackend::default(),
        None,
    );
    let mut client2 = AxMdWebSocketClient::new(
        ws_url,
        "token2".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client1.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await >= 1 },
        Duration::from_secs(5),
    )
    .await;

    client2.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await >= 2 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(*state.connection_count.lock().await, 2);

    client1.close().await;
    client2.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_client_debug() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    let debug_str = format!("{client:?}");
    assert!(debug_str.contains("AxMdWebSocketClient"));
    assert!(debug_str.contains("ws://localhost:9999/md/ws"));
}

#[rstest]
#[tokio::test]
async fn test_orders_client_debug() {
    let account_id = AccountId::from("AX-001");
    let trader_id = TraderId::from("TESTER-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        trader_id,
        30,
        TransportBackend::default(),
        None,
    );

    let debug_str = format!("{client:?}");
    assert!(debug_str.contains("AxOrdersWebSocketClient"));
    assert!(debug_str.contains("ws://localhost:9999/orders/ws"));
}

#[rstest]
#[tokio::test]
async fn test_md_rapid_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    for _ in 0..3 {
        let mut client = AxMdWebSocketClient::new(
            ws_url.clone(),
            "test_token".to_string(),
            30,
            TransportBackend::default(),
            None,
        );

        client.connect().await.unwrap();
        wait_for_connection(&state).await;

        assert!(client.is_active());

        client.close().await;

        wait_until_async(
            || async { *state.connection_count.lock().await == 0 },
            Duration::from_secs(5),
        )
        .await;
    }
}

#[rstest]
#[tokio::test]
async fn test_md_many_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        30,
        TransportBackend::default(),
        None,
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    let symbols = [
        "EURUSD-PERP",
        "ETHUSD-PERP",
        "AUDUSD-PERP",
        "GBPUSD-PERP",
        "USDJPY-PERP",
    ];

    for symbol in symbols {
        client
            .subscribe_book_deltas(symbol, AxMarketDataLevel::Level1)
            .await
            .unwrap();
    }

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= symbols.len() },
        Duration::from_secs(10),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(subs.len() >= symbols.len());

    client.close().await;
}
