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
//! [CoinAPI Market Data API](https://www.coinapi.io/products/market-data-api?ref=nautilus),
//! provided by API Bricks.
//!
//! This crate provides authenticated REST and WebSocket access, native venue instrument definitions,
//! live market data, historical requests, and loaders for downloaded flat files.
//! Order execution and account management are unsupported.
//!
//! [`http::client::CoinApiHttpClient`] returns typed provider results, and
//! [`websocket::client::CoinApiWebSocketClient`] exposes raw market-data streams.
//! [`data::CoinApiDataClient`] uses explicit instrument selections and shares a finite lifetime REST
//! credit allowance across all REST requests, including retries. Metadata and current book snapshots
//! cost one credit per attempt. Historical requests can return truncated results when that allowance
//! runs out. CoinAPI and native adapters must load disjoint instrument sets within a node; separate
//! client IDs do not isolate definitions or feeds.
//!
//! L2 market-by-price (MBP) and L3 market-by-order (MBO) deltas include snapshot gating and sequence
//! gap recovery. Every book subscription shares one WebSocket book channel; L3 requires individual
//! order IDs. Bars publish when a later period arrives, and reconnects discard retained periods to
//! avoid incomplete bars. Historical book snapshots are unsupported.
//!
//! The [`csv`] module loads downloaded book, quote, trade, and OHLCV exports without downloading them.
//! Optional Python bindings expose provider identity, configuration, factories, and flat-file loaders.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
//! engine for multi-asset, multi-venue trading systems.
//!
//! The system spans research, deterministic simulation, and live execution within a single
//! event-driven architecture, providing research-to-live semantic parity.
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
//! - `fuzz`: Enables libFuzzer integration for fuzz targets.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings for identity, configuration, factories, and flat-file loaders
//!   through [PyO3](https://pyo3.rs).
//! - `simulation`: Routes HTTP and WebSocket through deterministic transport when built with
//!   `cfg(madsim)`.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod book;
pub mod common;
pub mod config;
pub mod csv;
pub mod data;
pub mod factories;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;
