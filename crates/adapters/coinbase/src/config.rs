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

//! Configuration for Coinbase adapter.

use serde::{Deserialize, Serialize};

/// Configuration for Coinbase HTTP client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinbaseHttpConfig {
    /// API key
    pub api_key: String,
    /// API secret
    pub api_secret: String,
    /// Base URL (optional, defaults to production)
    pub base_url: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: Option<u64>,
}

impl CoinbaseHttpConfig {
    /// Create a new configuration
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key,
            api_secret,
            base_url: None,
            timeout_secs: Some(30),
        }
    }

    /// Set base URL
    #[must_use]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(base_url);
        self
    }

    /// Set timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = Some(timeout_secs);
        self
    }
}

/// Configuration for Coinbase WebSocket client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinbaseWebSocketConfig {
    /// API key
    pub api_key: String,
    /// API secret
    pub api_secret: String,
    /// WebSocket URL (optional, defaults to production)
    pub ws_url: Option<String>,
}

impl CoinbaseWebSocketConfig {
    /// Create a new configuration
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key,
            api_secret,
            ws_url: None,
        }
    }

    /// Set WebSocket URL
    #[must_use]
    pub fn with_ws_url(mut self, ws_url: String) -> Self {
        self.ws_url = Some(ws_url);
        self
    }
}

