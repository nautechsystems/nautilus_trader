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

//! WebSocket client implementation for the dYdX v4 API.
//!
//! This module provides real-time streaming connectivity to dYdX WebSocket endpoints,
//! supporting:
//!
//! - **Market data streaming**: Trades, order books, candles (bars), and market updates (oracle prices).
//! - **Private data streaming**: Subaccount updates, orders, fills, and positions.
//! - **Channel subscription management**: Subscribe and unsubscribe to public and private channels.
//! - **Automatic reconnection**: Reconnection with state restoration and resubscription.
//! - **Message parsing**: Fast conversion of WebSocket messages to Nautilus domain objects.
//!
//! # Architecture
//!
//! The WebSocket client follows a two-layer architecture:
//!
//! - **Outer client** ([`client::DydxWebSocketClient`]): Orchestrates connection lifecycle, manages
//!   subscriptions, and maintains state accessible to Python via `Arc<DashMap>`.
//! - **Inner handler** ([`handler::FeedHandler`]): Runs in a dedicated Tokio task as the I/O boundary,
//!   processing commands and parsing raw WebSocket messages into Nautilus types.
//!
//! Communication between layers uses lock-free channels:
//! - Commands flow from client to handler via `mpsc` channel.
//! - Parsed domain events flow from handler to client via `mpsc` channel.
//!
//! # References
//!
//! - dYdX v4 WebSocket API: <https://docs.dydx.trade/developers/indexer/websockets>

pub mod client;
pub mod enums;
pub mod error;
pub mod handler;
pub mod messages;
pub mod parse;
pub mod types;

// Re-exports
pub use client::DydxWebSocketClient;
pub use enums::{DydxWsChannel, DydxWsOperation};
pub use error::{DydxWebSocketError, DydxWsError, DydxWsResult};
pub use messages::{DydxWsMessage, NautilusWsMessage};
