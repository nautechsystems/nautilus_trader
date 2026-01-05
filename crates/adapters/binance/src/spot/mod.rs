// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Binance Spot market adapter with full SBE (Simple Binary Encoding) support.
//!
//! This module provides high-performance market data and execution clients for
//! Binance Spot markets using SBE encoding for both REST API and WebSocket streams.
//!
//! ## Features
//!
//! - **SBE REST API**: All REST responses decoded from SBE format.
//! - **SBE WebSocket Streams**: Market data streams with microsecond timestamps.
//! - **Ed25519 Authentication**: Required for SBE market data streams.
//!
//! ## Architecture
//!
//! ```text
//! spot/
//! ├── http/               # REST API client (SBE encoded)
//! │   ├── client.rs       # BinanceSpotHttpClient
//! │   ├── models.rs       # Response types
//! │   └── query.rs        # Query parameter builders
//! └── websocket/          # WebSocket clients (SBE encoded)
//!     ├── streams/        # Market data streams (pub/sub)
//!     │   ├── client.rs   # BinanceSpotWebSocketClient
//!     │   └── handler.rs  # Message parsing and routing
//!     └── trading/        # Trading API (request/response)
//!         ├── client.rs   # BinanceSpotWsTradingClient
//!         └── handler.rs  # Request/response handling
//! ```

pub mod enums;
pub mod http;
pub mod websocket;

// Re-export main client types
pub use enums::{BinanceCancelReplaceMode, BinanceOrderResponseType, BinanceSpotOrderType};
pub use http::client::BinanceSpotHttpClient;
pub use websocket::{BinanceSpotWebSocketClient, BinanceSpotWsTradingClient};
