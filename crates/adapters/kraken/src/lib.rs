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

//! Kraken exchange adapter for NautilusTrader.
//!
//! This adapter provides integration with the Kraken cryptocurrency exchange,
//! supporting both Spot and Futures markets.
//!
//! # Features
//!
//! - REST API v2 client for market data and account operations
//! - WebSocket v2 client for real-time data feeds
//! - Support for Spot and Futures markets
//! - Comprehensive instrument, ticker, trade, orderbook, and OHLC data
//! - Prepared for execution support (orders, positions, balances)
//!
//! # API Documentation
//!
//! - [Kraken REST API](https://docs.kraken.com/api/)
//! - [Kraken WebSocket v2](https://docs.kraken.com/websockets-v2/)
//!
//! # Python Bindings
//!
//! Enable the `python` feature to use this adapter from Python:
//!
//! ```toml
//! nautilus-kraken = { version = "0.52.0", features = ["python"] }
//! ```

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;

pub use config::{KrakenDataClientConfig, KrakenExecClientConfig};
pub use http::client::{KrakenHttpClient, KrakenRawHttpClient};
pub use websocket::client::KrakenWebSocketClient;
