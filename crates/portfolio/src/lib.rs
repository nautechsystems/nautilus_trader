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

//! Portfolio management and risk analysis for [NautilusTrader](http://nautilustrader.io).
//!
//! The `nautilus-portfolio` crate provides comprehensive portfolio management capabilities including
//! real-time position tracking, performance calculations, and risk management. This includes
//! sophisticated portfolio analytics and multi-currency support:
//!
//! - **Portfolio tracking**: Real-time portfolio state management with position and balance monitoring.
//! - **Account management**: Support for cash and margin accounts across multiple venues.
//! - **Performance calculations**: Real-time unrealized PnL, realized PnL, and mark-to-market valuations.
//! - **Risk management**: Initial margin calculations, maintenance margin tracking, and exposure monitoring.
//! - **Multi-currency support**: Currency conversion and cross-currency risk exposure analysis.
//! - **Configuration options**: Flexible settings for price types, currency conversion, and portfolio behavior.
//!
//! The crate handles complex portfolio scenarios including multi-venue trading, currency conversions,
//! and sophisticated margin calculations for both live trading and backtesting environments.
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

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod config;
pub mod manager;
pub mod portfolio;

#[cfg(test)]
mod tests;

// Re-exports
pub use portfolio::Portfolio;
