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

//! Network functionality for [NautilusTrader](http://nautilustrader.io).
//!
//! The `nautilus-network` crate provides networking components including HTTP, WebSocket, and raw TCP socket
//! clients, rate limiting, backoff strategies, and socket TLS utilities for connecting to
//! trading venues and data providers.
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! NautilusTrader's design, architecture, and implementation philosophy prioritizes software correctness and safety at the
//! highest level, with the aim of supporting mission-critical, trading system backtesting and live deployment workloads.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds the crate as a Python extension module.
//! - `turmoil`: Enables deterministic network simulation testing with [turmoil](https://github.com/tokio-rs/turmoil).
//!
//! # Testing
//!
//! The crate includes both standard integration tests and deterministic network simulation tests using turmoil.
//!
//! To run standard tests:
//! ```bash
//! cargo nextest run -p nautilus-network
//! ```
//!
//! To run turmoil network simulation tests:
//! ```bash
//! cargo nextest run -p nautilus-network --features turmoil
//! ```
//!
//! The turmoil tests simulate various network conditions (reconnections, partitions, etc.) in a deterministic way,
//! allowing reliable testing of network failure scenarios without flakiness.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod backoff;
pub mod fix;
pub mod http;
pub mod mode;
pub mod net;
pub mod retry;
pub mod socket;
pub mod websocket;

mod logging;
mod tls;

#[cfg(feature = "python")]
pub mod python;

pub mod error;
pub mod ratelimiter;

/// Sentinel message to signal reconnection to Rust consumers.
pub const RECONNECTED: &str = "__RECONNECTED__";
