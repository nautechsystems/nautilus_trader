use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// Unique identifier for a signer (API wallet or user address).
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignerId(pub String);

impl Display for SignerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SignerId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SignerId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Hyperliquid action types for different signing schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HyperliquidActionType {
    /// L1 actions (agent deposits, withdrawals) - signed with L1 scheme.
    L1,
    /// User actions (trading) - signed with user-signed scheme.
    UserSigned,
}
