//! Common data structures shared across the Kraken adapter.

use serde::{Deserialize, Serialize};

/// Generic Kraken API response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenResponse<T> {
    pub result: Option<T>,
    pub error: Option<Vec<String>>,
    #[serde(default)]
    pub success: bool,
}

impl<T> KrakenResponse<T> {
    /// Returns true if the response indicates success.
    pub fn is_success(&self) -> bool {
        self.success || (self.error.is_none() || self.error.as_ref().is_some_and(|e| e.is_empty()))
    }

    /// Returns the error message if present.
    pub fn error_message(&self) -> Option<String> {
        self.error
            .as_ref()
            .filter(|e| !e.is_empty())
            .map(|e| e.join(", "))
    }
}
