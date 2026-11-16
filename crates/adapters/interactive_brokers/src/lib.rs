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
//! The official Interactive Brokers TWS API reference can be found at
//! <https://ibkrcampus.com/campus/ibkr-api-page/twsapi-doc/>.
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
//! - `examples`: Enables the crate's example binaries.
//! - `execution` (default): Enables order execution and networking support.
//! - `extension-module`: Builds the crate as a Python extension module. This is the feature used by
//!   the `nautilus_trader` package and includes `python` and `gateway`.
//! - `gateway`: Enables the Dockerized IB Gateway manager via
//!   [`bollard`](https://crates.io/crates/bollard), including its PyO3 bindings when combined with
//!   `python`.
//! - `python`: Enables [PyO3](https://pyo3.rs) bindings for configs, enums, the historical
//!   client, and the instrument provider.
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/interactive_brokers/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/interactive_brokers/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers)

#![warn(rustc::all)]
#![deny(unsafe_code)]
// Clippy: allow style lints that would require large refactors across the adapter
#![allow(
    clippy::option_if_let_else,
    clippy::type_complexity,
    clippy::module_inception,
    clippy::result_large_err,
    clippy::bind_instead_of_map,
    clippy::too_many_arguments,
    clippy::missing_errors_doc,
    clippy::doc_overindented_list_items
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
pub mod data_types;
pub mod execution;
pub mod factories;
pub mod gateway;
pub mod historical;
pub mod providers;

#[cfg(feature = "python")]
pub mod python;

#[cfg(test)]
mod stubs;
