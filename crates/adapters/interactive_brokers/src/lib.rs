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

//! [NautilusTrader](http://nautilustrader.io) adapter for [Interactive Brokers](https://www.interactivebrokers.com).
//!
//! The `nautilus-interactive-brokers` crate provides client bindings for the Interactive Brokers
//! TWS API using the `rust-ibapi` library, with full integration into NautilusTrader's data and
//! execution infrastructure.
//!
//! # Platform
//!
//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module (used with `python`).

#![warn(rustc::all)]
#![deny(unsafe_code)]
// Clippy: allow style lints that would require large refactors across the adapter
#![allow(
    clippy::collapsible_if,
    clippy::if_not_else,
    clippy::uninlined_format_args,
    clippy::map_unwrap_or,
    clippy::redundant_clone,
    clippy::ignored_unit_patterns,
    clippy::items_after_statements,
    clippy::bool_to_int_with_if,
    clippy::cloned_instead_of_copied,
    clippy::option_if_let_else,
    clippy::type_complexity,
    clippy::await_holding_lock,
    clippy::module_inception,
    clippy::result_large_err,
    clippy::implicit_clone,
    clippy::single_char_pattern,
    clippy::bind_instead_of_map,
    clippy::explicit_iter_loop,
    clippy::too_many_arguments,
    clippy::missing_errors_doc,
    clippy::doc_overindented_list_items,
    clippy::needless_borrows_for_generic_args
)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod common;
pub mod config;
pub mod data;
pub mod error;
pub mod execution;
pub mod gateway;
pub mod historical;
pub mod providers;

#[cfg(feature = "python")]
pub mod python;
