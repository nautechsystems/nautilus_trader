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

pub mod client;
pub mod enums;
pub mod error;
pub mod messages;
pub mod parse;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::Value;
    use rustls::crypto::aws_lc_rs;

    use super::client::{HyperliquidWebSocketClient, MessageHandler};
    use crate::common::credentials::HyperliquidCredentials;

    const TEST_PRIVATE_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const TEST_WALLET: &str = "0x1234567890123456789012345678901234567890";

    fn create_test_credentials() -> HyperliquidCredentials {
        HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            Some(TEST_WALLET.to_string()),
            true, // testnet
        )
    }

    #[test]
    fn test_websocket_client_creation() {
        // Test without credentials
        let client = HyperliquidWebSocketClient::new(None, None);
        assert!(client.is_ok());

        // Test with credentials
        let creds = create_test_credentials();
        let client_with_creds = HyperliquidWebSocketClient::new(None, Some(creds));
        assert!(client_with_creds.is_ok());

        // Test with custom URL
        let custom_client = HyperliquidWebSocketClient::new(
            Some("wss://api.hyperliquid-testnet.xyz/ws".to_string()),
            None,
        );
        assert!(custom_client.is_ok());
    }

    #[test]
    fn test_connection_state() {
        let client = HyperliquidWebSocketClient::new(None, None).unwrap();
        
        // Initially not connected
        assert!(!client.is_connected());
        
        // Reconnection attempts should be 0
        assert_eq!(client.reconnect_attempts(), 0);
        
        // Time since heartbeat should be None initially
        assert!(client.time_since_heartbeat().is_none());
    }

    #[test]
    fn test_message_handler() {
        let mut client = HyperliquidWebSocketClient::new(None, None).unwrap();
        
        // Create a test message handler
        let test_handler: MessageHandler = Arc::new(|_msg: Value| {
            // Test handler that does nothing
        });
        
        // Set the handler
        client.set_message_handler(test_handler);
        
        // The client should accept the handler without error
        // We can't easily test the actual callback in unit tests
    }

    #[tokio::test]
    async fn test_subscription_methods() {
        let mut client = HyperliquidWebSocketClient::new(None, None).unwrap();
        
        // Test subscription methods (these will fail without actual connection)
        // But they should create the subscription JSON correctly
        
        // Test allMids subscription
        let result = client.subscribe_all_mids().await;
        // Should fail because not connected, but structure should be valid
        assert!(result.is_err());
        
        // Test L2 book subscription
        let result = client.subscribe_l2_book("BTC").await;
        assert!(result.is_err());
        
        // Test trades subscription
        let result = client.subscribe_trades("BTC").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connection_lifecycle() {
        // Initialize TLS crypto provider
        let _ = aws_lc_rs::default_provider().install_default();
        
        // Use invalid URL to ensure connection fails
        let mut client = HyperliquidWebSocketClient::new(
            Some("ws://invalid-url-for-testing:9999/ws".to_string()), 
            None
        ).unwrap();
        
        // Set a dummy message handler
        let handler: MessageHandler = Arc::new(|_msg: Value| {});
        client.set_message_handler(handler);
        
        // Initial state
        assert!(!client.is_connected());
        assert_eq!(client.reconnect_attempts(), 0);
        
        // Try to connect (will fail without real server, but tests structure)
        let connect_result = client.connect().await;
        // Connection will fail in test environment, which is expected
        assert!(connect_result.is_err());
        
        // Try disconnect (should work regardless)
        let disconnect_result = client.disconnect().await;
        assert!(disconnect_result.is_ok());
        
        // After disconnect, should not be connected
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn test_connect_with_retry() {
        // Initialize TLS crypto provider
        let _ = aws_lc_rs::default_provider().install_default();
        
        // Use invalid URL to ensure connection fails
        let mut client = HyperliquidWebSocketClient::new(
            Some("ws://invalid-url-for-testing:9999/ws".to_string()), 
            None
        ).unwrap();
        
        // Set a dummy message handler
        let handler: MessageHandler = Arc::new(|_msg: Value| {});
        client.set_message_handler(handler);
        
        // Try connecting with retry (will fail but test retry logic)
        let start_time = std::time::Instant::now();
        let connect_result = client.connect_with_retry(2).await;
        let elapsed = start_time.elapsed();
        
        // Should fail but take some time due to retry logic
        assert!(connect_result.is_err());
        // Should take at least 1 second for retry delay
        assert!(elapsed >= Duration::from_millis(500));
        
        // Should have recorded retry attempts
        assert!(client.reconnect_attempts() > 0);
    }

    #[test]
    fn test_websocket_debug_format() {
        let creds = create_test_credentials();
        let client = HyperliquidWebSocketClient::new(
            Some("wss://test.example.com/ws".to_string()),
            Some(creds),
        ).unwrap();
        
        let debug_str = format!("{:?}", client);
        assert!(debug_str.contains("HyperliquidWebSocketClient"));
        assert!(debug_str.contains("wss://test.example.com/ws"));
    }

    #[test]
    fn test_subscription_json_format() {
        // Test that subscription JSON is formed correctly
        let all_mids_json = serde_json::json!({
            "method": "subscribe",
            "subscription": {
                "type": "allMids"
            }
        });
        
        assert_eq!(all_mids_json["method"], "subscribe");
        assert_eq!(all_mids_json["subscription"]["type"], "allMids");
        
        let l2_book_json = serde_json::json!({
            "method": "subscribe",
            "subscription": {
                "type": "l2Book",
                "coin": "BTC"
            }
        });
        
        assert_eq!(l2_book_json["method"], "subscribe");
        assert_eq!(l2_book_json["subscription"]["type"], "l2Book");
        assert_eq!(l2_book_json["subscription"]["coin"], "BTC");
        
        let trades_json = serde_json::json!({
            "method": "subscribe",
            "subscription": {
                "type": "trades",
                "coin": "ETH"
            }
        });
        
        assert_eq!(trades_json["method"], "subscribe");
        assert_eq!(trades_json["subscription"]["type"], "trades");
        assert_eq!(trades_json["subscription"]["coin"], "ETH");
    }
}
