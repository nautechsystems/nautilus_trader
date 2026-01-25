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

//! Binance Spot WebSocket market data streams with SBE encoding.
//!
//! This module provides WebSocket connectivity to Binance's SBE market data streams
//! at `stream-sbe.binance.com`. These streams provide lower latency and smaller
//! payloads compared to JSON streams.
//!
//! ## Available Streams
//!
//! - `<symbol>@trade` - Real-time trade data
//! - `<symbol>@bestBidAsk` - Best bid/offer updates
//! - `<symbol>@depth` - Order book diff updates
//! - `<symbol>@depth20` - Top 20 order book levels
//!
//! ## Authentication
//!
//! SBE market data streams require Ed25519 API key authentication via the
//! `X-MBX-APIKEY` header.

pub mod client;
pub mod handler;
pub mod messages;
pub mod parse;
pub mod subscription;

pub use client::BinanceSpotWebSocketClient;
