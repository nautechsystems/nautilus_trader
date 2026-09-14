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

//! [NautilusTrader](https://nautilustrader.io) adapter for
//! [Interactive Brokers](https://www.interactivebrokers.com).
//!
//! The `nautilus-interactive-brokers` crate wraps the [`ibapi`](https://crates.io/crates/ibapi)
//! client and connects it to NautilusTrader's live data, execution, historical data, and
//! instrument loading infrastructure.
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
//! depending on the intended use case (Rust-only builds vs. Python bindings through PyO3).
//!
//! - `examples`: Enables the crate's example binaries.
//! - `execution` (default): Enables order execution and networking support.
//! - `extension-module`: Builds the crate as a Python extension module. This is the feature used by
//!   the `nautilus_trader` package and includes `python` and `gateway`.
//! - `gateway`: Enables the Dockerized IB Gateway manager via
//!   [`bollard`](https://crates.io/crates/bollard), including its PyO3 bindings when combined with
//!   `python`.
//! - `python`: Enables [PyO3](https://pyo3.rs) bindings for configs, enums, the historical
//!   client, and the instrument provider.

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
// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod common;
pub mod config;
pub mod data;
pub mod error;
pub mod execution;
pub mod factories;
pub mod gateway;
pub mod historical;
pub mod providers;

#[cfg(feature = "python")]
pub mod python;
