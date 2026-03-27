//! Adapter-specific enumerations.

use serde::{Deserialize, Serialize};

/// Gateway connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error,
}

/// Market data subscription type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketDataType {
    Quote,
    Trade,
    Depth,
}
