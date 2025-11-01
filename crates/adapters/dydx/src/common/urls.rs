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

//! URL constants for dYdX API endpoints.

// HTTP API URLs
/// dYdX v4 mainnet HTTP API base URL.
pub const DYDX_HTTP_URL: &str = "https://indexer.dydx.trade";

/// dYdX v4 testnet HTTP API base URL.
pub const DYDX_TESTNET_HTTP_URL: &str = "https://indexer.v4testnet.dydx.exchange";

// WebSocket URLs
/// dYdX v4 mainnet WebSocket URL.
pub const DYDX_WS_URL: &str = "wss://indexer.dydx.trade/v4/ws";

/// dYdX v4 testnet WebSocket URL.
pub const DYDX_TESTNET_WS_URL: &str = "wss://indexer.v4testnet.dydx.exchange/v4/ws";

// gRPC URLs
/// dYdX v4 mainnet gRPC URL (public node).
pub const DYDX_GRPC_URL: &str = "https://dydx-grpc.publicnode.com:443";

/// dYdX v4 testnet gRPC URL.
pub const DYDX_TESTNET_GRPC_URL: &str = "https://dydx-testnet-grpc.publicnode.com:443";
