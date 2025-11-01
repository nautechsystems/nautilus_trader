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

//! Configuration structures for the dYdX adapter.

use serde::{Deserialize, Serialize};

/// Configuration for the dYdX adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxAdapterConfig {
    /// Base URL for the HTTP API.
    pub base_url: String,
    /// Base URL for the WebSocket API.
    pub ws_url: String,
    /// Base URL for the gRPC API (Cosmos SDK transactions).
    pub grpc_url: String,
    /// Chain ID (e.g., "dydx-mainnet-1" for mainnet, "dydx-testnet-4" for testnet).
    pub chain_id: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for DydxAdapterConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.dydx.exchange".to_string(),
            ws_url: "wss://api.dydx.exchange/v4/ws".to_string(),
            grpc_url: "https://dydx-grpc.publicnode.com:443".to_string(),
            chain_id: "dydx-mainnet-1".to_string(),
            timeout_secs: 30,
        }
    }
}
