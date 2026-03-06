//! Core constants shared across the Kraken adapter components.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const KRAKEN: &str = "KRAKEN";
pub static KRAKEN_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(KRAKEN)));

// API Partner integration identifier
pub const NAUTILUS_KRAKEN_BROKER_ID: &str = "AA98 N84G GOPN GL6Y";

// WebSocket-specific constants
pub const KRAKEN_PONG: &str = "pong";
pub const KRAKEN_WS_TOPIC_DELIMITER: char = '.';

// Spot API URLs (v2)
pub const KRAKEN_SPOT_HTTP_URL: &str = "https://api.kraken.com";
pub const KRAKEN_SPOT_WS_PUBLIC_URL: &str = "wss://ws.kraken.com/v2";
pub const KRAKEN_SPOT_WS_PRIVATE_URL: &str = "wss://ws-auth.kraken.com/v2";

// Futures API URLs
pub const KRAKEN_FUTURES_HTTP_URL: &str = "https://futures.kraken.com";
pub const KRAKEN_FUTURES_WS_URL: &str = "wss://futures.kraken.com/ws/v1";

// Demo URLs
pub const KRAKEN_FUTURES_DEMO_HTTP_URL: &str = "https://demo-futures.kraken.com";
pub const KRAKEN_FUTURES_DEMO_WS_URL: &str = "wss://demo-futures.kraken.com/ws/v1";
