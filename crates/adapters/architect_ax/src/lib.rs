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

//! [NautilusTrader](https://nautilustrader.io) adapter for Ax's [AX Exchange](https://architect.exchange).
//!
//! [AX Exchange](https://architect.exchange) is the world's first centralized and regulated
//! exchange for perpetual futures on traditional underlying asset classes (FX, rates, metals,
//! energy, stock indexes). Designed for institutional and professional traders, it combines
//! innovations from digital asset perpetual exchanges with the safety and risk management of
//! traditional futures exchanges. Licensed under the [Bermuda Monetary Authority (BMA)](https://www.bma.bm).
//!
//! The `nautilus-architect-ax` crate connects AX Exchange market data and order execution to NautilusTrader.
//! It provides typed HTTP and WebSocket clients, instrument definitions, and live-node factories for
//! production and sandbox environments.
//!
//! The official AX Exchange API reference can be found at <https://docs.architect.exchange/api-reference>.
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
//! - `extension-module`: Builds as a Python extension module.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//!
//! # Integrator Attribution
//!
//! WebSocket order submissions set `tag` to `Nautilus` for attribution.
//! HTTP order submissions omit `tag` unless the caller supplies it.
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/architect_ax/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/architect_ax/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/architect_ax)

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
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod websocket;

mod book;

#[cfg(feature = "python")]
pub mod python;
