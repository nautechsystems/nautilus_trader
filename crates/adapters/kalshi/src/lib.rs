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

//! [NautilusTrader](https://nautilustrader.io) adapter for [Kalshi](https://kalshi.com), a regulated event contracts exchange.
//!
//! The `nautilus-kalshi` crate provides instrument loading, live and historical market data, and order execution
//! for binary event contracts through the **Kalshi API v2**. It includes typed HTTP and WebSocket clients,
//! account and execution reconciliation, and live-node factories.
//!
//! Execution recovery uses authoritative venue reports. Unknown write outcomes block further order commands until
//! reconciliation resolves them. Direct client reconnects continue report collection while writes remain blocked;
//! node startup waits for write readiness.
//!
//! The official Kalshi API reference can be found at <https://docs.kalshi.com/>.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
//! with a deterministic, event-driven architecture for multi-asset, multi-venue systems.
//!
//! The system spans research, simulation, and live execution, providing research-to-live semantic parity.
//!
//! # Feature Flags
//!
//! Feature flags control code inclusion for Rust builds and Python bindings:
//!
//! - `examples`: Enables book stress scenarios.
//! - `extension-module`: Builds as a Python extension module.
//! - `fuzz`: Enables libFuzzer integration for fuzz targets.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `simulation`: Routes HTTP through deterministic network seams with `cfg(madsim)`.
//!   Simulation supports plaintext local HTTP peers; TLS and proxies remain native-only.
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/kalshi/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/kalshi/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/kalshi)
//! - [Data and execution benchmarks](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/kalshi/benches/BENCHMARKS.md)

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod book;

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod providers;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;
