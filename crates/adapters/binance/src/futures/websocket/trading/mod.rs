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

//! Binance Futures WebSocket Trading API client.
//!
//! This module provides a WebSocket Trading API client for Binance USD-M Futures
//! using the JSON request/response pattern. It complements the HTTP client by
//! offering lower-latency order submission via WebSocket.
//!
//! ## Architecture
//!
//! The client uses a two-tier architecture:
//!
//! - **Outer client** (`BinanceFuturesWsTradingClient`): Orchestrates connection lifecycle,
//!   maintains state, sends commands via channel.
//! - **Inner handler** (`BinanceFuturesWsTradingHandler`): Runs in dedicated Tokio task,
//!   owns WebSocket connection, processes commands and JSON responses.
//!
//! ## Features
//!
//! - Order placement, cancellation, and modification via WebSocket
//! - JSON responses (unlike Spot which uses SBE binary)
//! - HMAC-SHA256 authentication per request
//! - Request/response correlation by ID

pub mod client;
pub mod error;
pub mod handler;
pub mod messages;

pub(crate) mod dispatch;

pub use client::BinanceFuturesWsTradingClient;
pub use error::{BinanceFuturesWsApiError, BinanceFuturesWsApiResult};
pub use handler::BinanceFuturesWsTradingHandler;
pub use messages::{BinanceFuturesWsTradingCommand, BinanceFuturesWsTradingMessage};
