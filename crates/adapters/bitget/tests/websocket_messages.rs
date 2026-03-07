// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use nautilus_bitget::{
    common::{enums::{BitgetEnvironment, BitgetProductType}, signing::ws_login_sign_base64},
    websocket::client::BitgetWebSocketClient,
};

#[test]
fn subscribe_message_escapes_untrusted_fields() {
    let channel = "books\"},\"channel\":\"hijack";
    let inst_id = "BTCUSDT\"}]";

    let message =
        BitgetWebSocketClient::subscribe_message(BitgetProductType::Spot, channel, inst_id);
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "subscribe");
    assert_eq!(parsed["args"][0]["instType"], "SPOT");
    assert_eq!(parsed["args"][0]["channel"], channel);
    assert_eq!(parsed["args"][0]["instId"], inst_id);
}

#[test]
fn unsubscribe_message_escapes_untrusted_fields() {
    let channel = "trade\"},\"channel\":\"hijack";
    let inst_id = "BTCUSDT\"}]";

    let message =
        BitgetWebSocketClient::unsubscribe_message(BitgetProductType::Spot, channel, inst_id);
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "unsubscribe");
    assert_eq!(parsed["args"][0]["instType"], "SPOT");
    assert_eq!(parsed["args"][0]["channel"], channel);
    assert_eq!(parsed["args"][0]["instId"], inst_id);
}

#[test]
fn subscribe_ticker_message_builds_ticker_channel() {
    let message = BitgetWebSocketClient::subscribe_ticker_message(BitgetProductType::Spot, "BTCUSDT");
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "subscribe");
    assert_eq!(parsed["args"][0]["instType"], "SPOT");
    assert_eq!(parsed["args"][0]["channel"], "ticker");
    assert_eq!(parsed["args"][0]["instId"], "BTCUSDT");
}

#[test]
fn unsubscribe_ticker_message_builds_ticker_channel() {
    let message =
        BitgetWebSocketClient::unsubscribe_ticker_message(BitgetProductType::Spot, "BTCUSDT");
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "unsubscribe");
    assert_eq!(parsed["args"][0]["instType"], "SPOT");
    assert_eq!(parsed["args"][0]["channel"], "ticker");
    assert_eq!(parsed["args"][0]["instId"], "BTCUSDT");
}

#[test]
fn subscribe_candle_message_prepares_candle_channel() {
    let message =
        BitgetWebSocketClient::subscribe_candle_message(BitgetProductType::UsdtFutures, "1m", "BTCUSDT");
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "subscribe");
    assert_eq!(parsed["args"][0]["instType"], "USDT-FUTURES");
    assert_eq!(parsed["args"][0]["channel"], "candle1m");
    assert_eq!(parsed["args"][0]["instId"], "BTCUSDT");
}

#[test]
fn unsubscribe_candle_message_prepares_candle_channel() {
    let message =
        BitgetWebSocketClient::unsubscribe_candle_message(BitgetProductType::UsdtFutures, "candle1m", "BTCUSDT");
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "unsubscribe");
    assert_eq!(parsed["args"][0]["instType"], "USDT-FUTURES");
    assert_eq!(parsed["args"][0]["channel"], "candle1m");
    assert_eq!(parsed["args"][0]["instId"], "BTCUSDT");
}

#[test]
fn ping_message_is_literal_ping() {
    assert_eq!(BitgetWebSocketClient::ping_message(), "ping");
}

#[test]
fn websocket_config_uses_default_bitget_websocket_runtime_defaults() {
    let ws_client = BitgetWebSocketClient::new(BitgetEnvironment::Mainnet);
    let ws_config = ws_client.websocket_config(None, None, None);

    assert_eq!(ws_config.url, "wss://ws.bitget.com/v2/ws/public");
    assert_eq!(ws_config.headers, vec![]);
    assert_eq!(ws_config.heartbeat, Some(30));
    assert_eq!(ws_config.heartbeat_msg.as_deref(), Some("ping"));
    assert_eq!(ws_config.reconnect_timeout_ms, Some(10_000));
    assert_eq!(ws_config.reconnect_delay_initial_ms, Some(2_000));
    assert_eq!(ws_config.reconnect_delay_max_ms, Some(30_000));
}

#[test]
fn websocket_config_accepts_custom_overrides() {
    let ws_client = BitgetWebSocketClient::new(BitgetEnvironment::Demo);
    let ws_config = ws_client.websocket_config(
        Some("wss://custom.example".to_string()),
        Some(5_000),
        Some(15_000),
    );

    assert_eq!(ws_config.url, "wss://custom.example");
    assert_eq!(ws_config.reconnect_delay_initial_ms, Some(5_000));
    assert_eq!(ws_config.reconnect_delay_max_ms, Some(15_000));
}

#[test]
fn login_message_matches_bitget_private_login_schema() {
    let api_key = "key\"with\"quotes";
    let passphrase = "pass\"phrase";
    let secret = "super-secret";
    let timestamp_ms = 1_708_883_200_123_i64;

    let message = BitgetWebSocketClient::login_message(api_key, passphrase, secret, timestamp_ms);
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "login");
    assert_eq!(parsed["args"][0]["apiKey"], api_key);
    assert_eq!(parsed["args"][0]["passphrase"], passphrase);
    assert_eq!(parsed["args"][0]["timestamp"], timestamp_ms.to_string());
    assert_eq!(parsed["args"][0]["sign"], ws_login_sign_base64(secret, timestamp_ms));
}

#[test]
fn subscribe_account_message_matches_bitget_private_account_schema() {
    let coin = "default";

    let message = BitgetWebSocketClient::subscribe_account_message(BitgetProductType::Spot, coin);
    let parsed: serde_json::Value = serde_json::from_str(&message).expect("valid JSON payload");

    assert_eq!(parsed["op"], "subscribe");
    assert_eq!(parsed["args"][0]["instType"], "SPOT");
    assert_eq!(parsed["args"][0]["channel"], "account");
    assert_eq!(parsed["args"][0]["coin"], coin);
}
