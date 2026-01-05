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

//! Binance Spot WebSocket API client with SBE (Simple Binary Encoding) support.
//!
//! This module provides a WebSocket API client for Binance Spot trading using
//! the request/response pattern with SBE-encoded responses. It complements the
//! HTTP client by offering lower-latency order submission via WebSocket.
//!
//! ## Architecture
//!
//! The client uses a two-tier architecture per the adapter design guidelines:
//!
//! - **Outer client** (`BinanceSpotWsTradingClient`): Orchestrates connection lifecycle,
//!   maintains state for Python access, sends commands via channel.
//! - **Inner handler** (`BinanceSpotWsApiHandler`): Runs in dedicated Tokio task,
//!   owns WebSocket connection, processes commands and responses.
//!
//! ## Features
//!
//! - Order placement, cancellation, and modification via WebSocket
//! - SBE-encoded responses for consistency with HTTP client
//! - Ed25519 authentication
//! - Request/response correlation by ID

pub mod client;
pub mod error;
pub mod handler;
pub mod messages;

pub use client::BinanceSpotWsTradingClient;
pub use error::{BinanceWsApiError, BinanceWsApiResult};
pub use handler::BinanceSpotWsApiHandler;
pub use messages::{HandlerCommand, NautilusWsApiMessage};
