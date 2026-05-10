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

//! Technical analysis indicators for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-indicators` crate provides a collection of technical analysis indicators
//! for quantitative trading and market research. This includes a wide variety of indicators
//! organized by category, with a unified trait-based architecture for consistent usage:
//!
//! - **Moving averages**: SMA, EMA, DEMA, HMA, WMA, VWAP, adaptive averages, and linear regression.
//! - **Momentum indicators**: RSI, MACD, Aroon, Bollinger Bands, CCI, Stochastics, and rate of change.
//! - **Volatility indicators**: ATR, Donchian Channels, Keltner Channels, and volatility ratios.
//! - **Ratio analysis**: Efficiency ratios and spread analysis for relative performance.
//! - **Order book indicators**: Book imbalance ratio for analyzing market microstructure.
//! - **Common indicator trait**: Unified interface supporting bars, quotes, trades, and order book data.
//!
//! All indicators are designed for high-performance real-time processing with bounded memory
//! usage and efficient circular buffer implementations. The crate supports both Rust-native
//! usage and Python integration for strategy development and backtesting.
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

pub mod average;
pub mod book;
pub mod indicator;
pub mod momentum;
pub mod ratio;
pub mod testing;
pub mod volatility;

#[cfg(test)]
mod stubs;

#[cfg(feature = "python")]
pub mod python;
