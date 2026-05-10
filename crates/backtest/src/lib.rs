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

//! Backtest engine for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-backtest` crate provides an event-driven backtesting framework that allows
//! quantitative traders to test and validate trading strategies on historical data with high
//! fidelity market simulation. The system replicates real market conditions including:
//!
//! - Event-driven backtesting engine with simulated exchanges.
//! - Market data replay with configurable latency and fill models.
//! - Order matching engines with realistic execution simulation.
//! - Multi-venue and multi-asset backtesting capabilities.
//! - Configuration and state management.
//! - Integration with live trading systems for direct deployment.
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
//! - `examples`: Enables example strategies and the EMA crossover backtest example.
//! - `streaming`: Enables `persistence` dependency for streaming configuration.
//! - `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds the crate as a Python extension module.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod accumulator;
pub mod config;
pub mod data_client;
pub mod data_iterator;
pub mod engine;
pub mod exchange;
pub mod execution_client;
pub mod modules;
pub mod result;

#[cfg(feature = "streaming")]
pub mod node;

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "ffi")]
pub mod ffi;
