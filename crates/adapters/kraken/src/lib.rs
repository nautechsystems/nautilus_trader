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
//! The `nautilus-kraken` crate provides separate Spot and Futures data and execution clients over HTTP
//! and WebSocket. It includes typed API models, instrument loading, order and account updates, and
//! live-node factories for both markets.
//!
//! The official Kraken API reference can be found at <https://docs.kraken.com/>.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
//! with a deterministic, event-driven architecture for multi-asset, multi-venue systems.
//!
//! The system spans research, simulation, and live execution, providing research-to-live semantic parity.
//!
//! # Features
//!
//! - REST API v2 client for market data and account operations.
//! - WebSocket v2 client for real-time data feeds.
//! - Support for Spot and Futures markets.
//! - Instrument, ticker, trade, orderbook, and OHLC data.
//! - Prepared for execution support (orders, positions, balances).
//!
//! # Feature Flags
//!
//! Feature flags control code inclusion for Rust builds and Python bindings:
//!
//! - `examples`: Enables the crate's example binaries.
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types. Keep this default for Futures because Kraken may return instrument
//!   precision above standard-precision mode's nine-decimal limit.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//!
//! # Integrator Attribution
//!
//! Spot and Futures HTTP order builders set `broker` to NautilusTrader's integration ID.
//! Raw HTTP clients preserve caller-provided parameters, and Spot WebSocket orders omit attribution.
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/kraken/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/kraken/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/kraken)

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
