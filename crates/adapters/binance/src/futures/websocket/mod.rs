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

//! Binance Futures WebSocket clients.
//!
//! This module provides WebSocket clients for Binance Futures:
//!
//! ## Market Data and User Data Streams (`streams`)
//!
//! Pub/sub pattern for real-time market data and user data via JSON WebSocket:
//! - Trade streams, order book updates, mark price, klines
//! - User data stream (order updates, account updates) via listenKey
//!
//! ## Trading API (`trading`)
//!
//! Request/response pattern for order management via `ws-fapi.binance.com`:
//! - Order placement, cancellation, and modification
//! - USD-M only (COIN-M does not support WebSocket Trading API)

pub mod streams;
pub mod trading;

pub use streams::BinanceFuturesWebSocketClient;
pub use trading::BinanceFuturesWsTradingClient;
