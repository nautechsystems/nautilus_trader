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

//! [NautilusTrader](https://nautilustrader.io) adapter for the
//! [Kraken](https://www.kraken.com) exchange.
//!
//! This adapter supports both Spot and Futures markets.
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
//! - [Kraken REST API](https://docs.kraken.com/api/docs/)
//! - [Kraken WebSocket v2](https://docs.kraken.com/api/docs/websocket-v2/)
//!
//! # Feature Flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `examples`: Enables the crate's example binaries.
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types. Keep this default for Futures because Kraken may return instrument
//!   precision above standard-precision mode's nine-decimal limit.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;

pub use config::{KrakenDataClientConfig, KrakenExecutionClientConfig};
pub use data::{KrakenFuturesDataClient, KrakenSpotDataClient};
pub use execution::{KrakenFuturesExecutionClient, KrakenSpotExecutionClient};
pub use http::{
    KrakenFuturesHttpClient, KrakenFuturesRawHttpClient, KrakenHttpError, KrakenSpotHttpClient,
    KrakenSpotRawHttpClient,
};
pub use websocket::{
    futures::client::KrakenFuturesWebSocketClient, spot_v2::client::KrakenSpotWebSocketClient,
};
