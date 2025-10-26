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

//! WebSocket client implementation for the OKX v5 API.
//!
//! This module provides real-time streaming connectivity to OKX WebSocket endpoints,
//! supporting:
//!
//! - Market data streaming (order books, trades, tickers, bars).
//! - Private data streaming (account updates, positions, orders).
//! - Order management (place, cancel, amend) via WebSocket.
//! - Authentication and automatic reconnection.
//! - Channel subscription management.

pub mod auth;
pub mod client;
pub mod enums;
pub mod error;
pub mod messages;
pub mod parse;
pub mod subscription;

// Re-exports
pub use crate::websocket::client::OKXWebSocketClient;
