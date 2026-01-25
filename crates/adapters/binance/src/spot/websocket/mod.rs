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

//! Binance Spot WebSocket clients with SBE (Simple Binary Encoding) support.
//!
//! This module provides two WebSocket clients for Binance Spot:
//!
//! ## Market Data Streams (`streams`)
//!
//! Pub/sub pattern for real-time market data via `stream-sbe.binance.com`:
//! - Trade streams
//! - Best bid/offer updates
//! - Order book depth updates
//!
//! ## Trading API (`trading`)
//!
//! Request/response pattern for order management via `ws-api.binance.com`:
//! - Order placement
//! - Order cancellation
//! - Cancel-replace operations

pub mod error;
pub mod streams;
pub mod trading;

pub use streams::BinanceSpotWebSocketClient;
pub use trading::BinanceSpotWsTradingClient;
