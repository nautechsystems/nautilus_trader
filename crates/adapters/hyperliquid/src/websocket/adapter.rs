//! Exchange adapter trait and factory for WebSocket message translation.
//! Provides a clean abstraction layer between canonical message types and exchange-specific formats.

use serde_json::Value;

use crate::websocket::codec::{WsInbound, WsOutbound};

/// Trait for exchange-specific WebSocket message adapters.
///
/// Adapters translate between canonical message types and vendor-specific wire formats,
/// providing a consistent interface while preserving exchange-specific semantics.
pub trait ExchangeAdapter: Send + Sync {
    /// Encode canonical outbound message to exchange-specific JSON.
    ///
    /// Returns `Value::Array` for exchanges that require multiple frames per logical message,
    /// or a single `Value::Object` for single-frame messages.
    fn encode(&self, msg: &WsOutbound) -> Value;

    /// Decode exchange-specific text frame to canonical inbound message.
    ///
    /// Returns `None` if the message cannot be parsed or is not recognized.
    fn decode(&self, txt: &str) -> Option<WsInbound>;
}

/// Factory function to create adapter based on URL.
pub fn adapter_for(url: &str) -> Box<dyn ExchangeAdapter> {
    if url.contains("hyperliquid") {
        Box::new(super::exchange::HyperliquidAdapter::new())
    } else {
        // For future exchanges (OKX, BitMEX, etc.)
        unimplemented!("No adapter available for URL: {}", url)
    }
}
