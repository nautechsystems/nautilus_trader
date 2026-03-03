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

//! Kraken exchange adapter for NautilusTrader.
//!
//! This adapter provides integration with the Kraken cryptocurrency exchange,
//! supporting both Spot and Futures markets.
//!
//! # Features
//!
//! - REST API v2 client for market data and account operations.
//! - WebSocket v2 client for real-time data feeds.
//! - Support for Spot and Futures markets.
//! - Instrument, ticker, trade, orderbook, and OHLC data.
//! - Prepared for execution support (orders, positions, balances).
//!
//! # API Documentation
//!
//! - [Kraken REST API](https://docs.kraken.com/api/)
//! - [Kraken WebSocket v2](https://docs.kraken.com/websockets-v2/)
//!
//! # Feature Flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module.
//!
//! [High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;

pub use config::{KrakenDataClientConfig, KrakenExecClientConfig};
pub use data::{KrakenFuturesDataClient, KrakenSpotDataClient};
pub use execution::{KrakenFuturesExecutionClient, KrakenSpotExecutionClient};
pub use http::{
    KrakenFuturesHttpClient, KrakenFuturesRawHttpClient, KrakenHttpError, KrakenSpotHttpClient,
    KrakenSpotRawHttpClient,
};
pub use websocket::{
    futures::client::KrakenFuturesWebSocketClient, spot_v2::client::KrakenSpotWebSocketClient,
};
