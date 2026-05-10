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
//! [Binance](https://www.binance.com/) cryptocurrency exchange.
//!
//! The `nautilus-binance` crate provides client bindings (HTTP & WebSocket), data
//! models, and helper utilities that wrap the official **Binance API**, covering:
//!
//! - Spot trading (api.binance.com)
//! - Spot margin trading
//! - USD-M Futures (fapi.binance.com)
//! - COIN-M Futures (dapi.binance.com)
//! - European Options (eapi.binance.com)
//!
//! The official Binance API reference can be found at <https://binance-docs.github.io/apidocs/>.
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
//! - `python`: Enables Python bindings via [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module (used together with `python`).
//!
//! [High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.
//!
//! # Documentation
//!
//! See <https://docs.rs/nautilus-binance> for the latest API documentation.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod arrow;
pub mod common;
pub mod config;
pub mod factories;
pub mod futures;
pub mod spot;

#[cfg(feature = "python")]
pub mod python;
