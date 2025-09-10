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

//! Configuration for Delta Exchange adapter.

use std::time::Duration;

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

/// Configuration for Delta Exchange HTTP client.
#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
#[builder(setter(into, strip_option), default)]
pub struct DeltaExchangeHttpConfig {
    /// The base URL for the HTTP API.
    pub base_url: String,
    /// The timeout for HTTP requests.
    pub timeout: Duration,
    /// Whether to use testnet environment.
    pub testnet: bool,
}

impl Default for DeltaExchangeHttpConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.delta.exchange".to_string(),
            timeout: Duration::from_secs(60),
            testnet: false,
        }
    }
}

/// Configuration for Delta Exchange WebSocket client.
#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
#[builder(setter(into, strip_option), default)]
pub struct DeltaExchangeWebSocketConfig {
    /// The base URL for the WebSocket API.
    pub base_url: String,
    /// The timeout for WebSocket connections.
    pub timeout: Duration,
    /// Whether to use testnet environment.
    pub testnet: bool,
    /// Maximum number of reconnection attempts.
    pub max_reconnection_attempts: u32,
    /// Delay between reconnection attempts.
    pub reconnection_delay: Duration,
}

impl Default for DeltaExchangeWebSocketConfig {
    fn default() -> Self {
        Self {
            base_url: "wss://socket.delta.exchange".to_string(),
            timeout: Duration::from_secs(30),
            testnet: false,
            max_reconnection_attempts: 10,
            reconnection_delay: Duration::from_secs(5),
        }
    }
}
