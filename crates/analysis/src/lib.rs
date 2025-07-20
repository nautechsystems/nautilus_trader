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

//! Portfolio analysis and performance metrics for [NautilusTrader](http://nautilustrader.io).
//!
//! The *analysis* crate provides a comprehensive suite of portfolio analysis tools and performance
//! statistics for evaluating trading strategies and portfolios. This includes return-based metrics,
//! PnL-based statistics, and risk measurements commonly used in quantitative finance:
//!
//! - Portfolio analyzer for tracking account states and positions.
//! - Extensive collection of performance statistics and risk metrics.
//! - Flexible statistic calculation framework supporting different data sources.
//! - Support for multi-currency portfolios and unrealized PnL calculations.
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

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod analyzer;
pub mod statistic;
pub mod statistics;

#[cfg(feature = "python")]
pub mod python;

use std::collections::BTreeMap;

use nautilus_core::UnixNanos;

/// Type alias for time-indexed returns data used in portfolio analysis.
///
/// Maps timestamps to return values for time-series analysis of portfolio performance.
pub type Returns = BTreeMap<UnixNanos, f64>;
