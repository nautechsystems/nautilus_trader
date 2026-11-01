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
//! [FinFeedAPI Prediction Markets API](https://www.finfeedapi.com/products/prediction-markets-api?ref=nautilus),
//! provided by API Bricks.
//!
//! This crate provides authenticated REST access, checked Polymarket binary Yes/No instrument
//! definitions, historical data requests, and book, quote, trade, and closed-bar polling.
//! Order execution is unsupported.
//!
//! [`http::client::FinFeedHttpClient`] returns typed provider results.
//! [`data::FinFeedDataClient`] uses explicit instrument selections and shares a finite request
//! allowance across bootstrap, history, polling, and retries. Polling provides no completeness or
//! automatic backfill guarantee. Historical L2 market-by-price (MBP) replay requires a returned
//! snapshot seed and relies on source action and continuity assumptions. OHLCV `LAST` does not
//! establish trade-only prices, and no authoritative settlement event is inferred.
//!
//! Optional Python bindings expose provider identity, configuration, factories, and read-only REST
//! access with typed queries and results.
//!
//! # Feature Flags
//!
//! - `extension-module`: Builds as a Python extension module.
//! - `fuzz`: Enables the REST decoder and domain conversion libFuzzer target.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings for identity, configuration, factories, and read-only REST access
//!   with typed queries and results through [PyO3](https://pyo3.rs).
//! - `simulation`: Routes time and HTTP transport through deterministic simulation with `cfg(madsim)`.
//!   Simulated transport supports plaintext local peers; HTTPS and explicit proxies require native tests.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod common;
pub mod config;
pub mod data;
pub mod factories;
pub mod http;
pub mod instruments;

#[cfg(feature = "python")]
pub mod python;
