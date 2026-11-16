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

//! [NautilusTrader](https://nautilustrader.io) adapter for the
//! [Binance](https://www.binance.com) cryptocurrency exchange.
//!
//! The `nautilus-binance` crate provides client bindings (HTTP & WebSocket) and data
//! models for the official **Binance API**. Live data and
//! execution clients are available for:
//!
//! - Spot markets, including Binance US (api.binance.com)
//! - USD-M Futures (fapi.binance.com)
//! - COIN-M Futures (dapi.binance.com)
//!
//! The crate also includes shared enums, endpoint constants, URL routing, and
//! credential plumbing for adjacent Binance surfaces such as Margin and European Options.
//! Those surfaces do not have live data or execution clients in this crate.
//! Portfolio Margin likewise has no live clients here, but its signed REST transport
//! is available in [`papi`].
//!
//! The official Binance API reference can be found at <https://developers.binance.com/en/docs/>.
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
//! - `arrow`: Enables Apache Arrow data support.
//! - `examples`: Enables the crate's example binaries.
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//!
//! # Integrator Attribution
//!
//! Spot and Futures execution clients and HTTP clients that build orders from Nautilus types encode
//! supported client order IDs with NautilusTrader's Link and Trade integration prefix. Raw HTTP and
//! WebSocket API trading clients preserve caller-supplied IDs.
//!
//! See [attribution details](https://nautilustrader.io/docs/nightly/integrations/binance/#link--trade).
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/binance/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/binance/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/binance)

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod common;
pub mod config;
pub mod data_types;
pub mod factories;
pub mod futures;
pub mod papi;
pub mod spot;

pub(crate) mod book;

#[cfg(feature = "arrow")]
pub mod arrow;

#[cfg(feature = "python")]
pub mod python;
