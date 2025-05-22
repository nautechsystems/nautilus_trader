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

/// Configuration for blockchain adapter connections.
#[derive(Debug, Clone)]
pub struct BlockchainAdapterConfig {
    /// Determines if the adapter should use Hypersync for live data streaming.
    pub use_hypersync_for_live_data: bool,
    /// The HTTP URL for the blockchain RPC endpoint.
    pub http_rpc_url: String,
    /// The maximum number of RPC requests allowed per second.
    pub rpc_requests_per_second: Option<u32>,
    /// The WebSocket secure URL for the blockchain RPC endpoint.
    pub wss_rpc_url: Option<String>,
}

impl BlockchainAdapterConfig {
    /// Creates a new [`BlockchainAdapterConfig`] instance.
    #[must_use]
    pub const fn new(
        http_rpc_url: String,
        rpc_requests_per_second: Option<u32>,
        wss_rpc_url: Option<String>,
        use_hypersync_for_live_data: bool,
    ) -> Self {
        Self {
            use_hypersync_for_live_data,
            http_rpc_url,
            rpc_requests_per_second,
            wss_rpc_url,
        }
    }
}
