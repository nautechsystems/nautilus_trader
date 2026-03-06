//! Core constants shared across the AX Exchange adapter components.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const AX: &str = "AX";
pub static AX_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(AX)));

/// Order tag identifying orders placed by NautilusTrader.
pub const AX_NAUTILUS_TAG: &str = "Nautilus";

// HTTP endpoints
pub const AX_HTTP_URL: &str = "https://gateway.architect.exchange/api";
pub const AX_HTTP_SANDBOX_URL: &str = "https://gateway.sandbox.architect.exchange/api";

// HTTP order management endpoints (separate base URL)
pub const AX_ORDERS_URL: &str = "https://gateway.architect.exchange/orders";
pub const AX_ORDERS_SANDBOX_URL: &str = "https://gateway.sandbox.architect.exchange/orders";

// Market data WebSocket endpoints
pub const AX_WS_PUBLIC_URL: &str = "wss://gateway.architect.exchange/md/ws";
pub const AX_WS_SANDBOX_PUBLIC_URL: &str = "wss://gateway.sandbox.architect.exchange/md/ws";

// Orders WebSocket endpoints (requires Bearer token authentication)
pub const AX_WS_PRIVATE_URL: &str = "wss://gateway.architect.exchange/orders/ws";
pub const AX_WS_SANDBOX_PRIVATE_URL: &str = "wss://gateway.sandbox.architect.exchange/orders/ws";

// Error message substrings for detecting specific rejection reasons
pub const AX_POST_ONLY_REJECT: &str = "Order may participate but not initiate in the market";
