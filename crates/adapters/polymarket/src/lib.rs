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

//! [NautilusTrader](https://nautilustrader.io) adapter for the [Polymarket](https://polymarket.com) prediction market.
//!
//! The `nautilus-polymarket` crate provides market data and order execution for binary outcome contracts
//! through the Polymarket CLOB APIs. It includes market discovery, authenticated HTTP and WebSocket
//! clients, order signing, and execution reconciliation, plus Deposit Wallet position operations
//! through the Relayer API.
//!
//! The official Polymarket API reference can be found at <https://docs.polymarket.com/>.
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
//! - `allowances`: Enables the `polymarket-set-allowances` binary.
//! - `examples`: Enables the crate's example binaries.
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//!
//! # Integrator Attribution
//!
//! Orders signed by the adapter carry the fixed NautilusTrader builder code with zero builder fees.
//!
//! See [attribution details](https://nautilustrader.io/docs/nightly/integrations/polymarket/#builder-code-attribution).
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/polymarket/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/polymarket/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/polymarket)

#![warn(rustc::all)]
#![deny(unsafe_code)]
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
pub mod filters;
pub mod http;
pub mod models;
pub mod positions;
pub mod providers;
pub mod session;
pub mod signing;
pub mod websocket;

mod book;
mod polybolt;
mod resolve;

#[cfg(feature = "python")]
pub mod python;
