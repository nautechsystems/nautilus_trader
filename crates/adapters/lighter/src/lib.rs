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

//! [NautilusTrader](https://nautilustrader.io) adapter for the [Lighter](https://lighter.xyz) DEX.
//!
//! The `nautilus-lighter` crate provides integration with the Lighter protocol for trading
//! perpetual futures and spot markets on the Lighter and Robinhood Chain deployments.
//!
//! The official Lighter API reference can be found at <https://docs.lighter.xyz/api>.
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
//! - `fuzz`: Enables libFuzzer integration for fuzz targets.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//!
//! Python bindings for the Lighter adapter are intentionally scoped to configuration,
//! enums, factory wiring, and integrator revocation. Data and execution
//! clients are consumed directly through the Rust trait surface.
//!
//! # Integrator Attribution
//!
//! On Lighter and Robinhood mainnets, the execution client adds NautilusTrader's integrator account index
//! to create and modify transactions with zero integrator fees. At startup it attempts to approve the
//! integrator and apply the `NAUTILUS` referral code. Both testnets omit attribution.
//!
//! See [attribution details](https://nautilustrader.io/docs/nightly/integrations/lighter/#integrator-attribution)
//! for approval, maker-only keys, and revocation.
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/lighter/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/lighter/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/lighter)

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
pub mod execution;
pub mod factories;
pub mod http;
pub mod signing;
pub mod websocket;

pub(crate) mod book;

#[cfg(feature = "python")]
pub mod python;

#[cfg(test)]
mod tests;
