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
//! The `nautilus-lighter` crate provides integration with the Lighter API for trading
//! perpetual futures and spot markets on a zk-rollup decentralized exchange.
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
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds as a Python extension module.
//!
//! Python bindings for the Lighter adapter are intentionally scoped to configuration,
//! enums, factory wiring, and integrator revocation. Data and execution
//! clients are consumed directly through the Rust trait surface.
//!
//! [High-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) (128-bit value types) is enabled by default.
//!
//! # Integrator attribution
//!
//! Submitted create and modify order transactions carry the NautilusTrader integrator account index
//! in Lighter's `L2TxAttributes`. This helps us gauge real usage of the integration and prioritize
//! ongoing maintenance. Maker and taker integrator fees are set to zero, so attribution adds no
//! trading cost.
//!
//! Lighter requires an `ApproveIntegrator` approval before these attributes can be attached to
//! orders. During startup, the execution client submits the required zero-fee approval for the
//! configured L2 account. See the
//! [Lighter integration guide](https://nautilustrader.io/docs/nightly/integrations/lighter.html#integrator-attribution)
//! for approval and revocation details.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod signing;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;
