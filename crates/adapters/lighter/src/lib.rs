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
//! # Integrator attribution
//!
//! NautilusTrader participates in [Lighter's partner program](https://apidocs.lighter.xyz/docs/partner-integration).
//! On Lighter Mainnet and Robinhood Mainnet, create and modify order transactions carry the
//! deployment-specific NautilusTrader integrator account index in `L2TxAttributes` across all
//! account tiers, including Standard. Maker and taker integrator fees are zero. Both testnets omit
//! integrator attributes.
//!
//! Lighter requires an `ApproveIntegrator` approval before these attributes can be attached to
//! mainnet orders. During startup, each mainnet execution client submits the required zero-fee
//! approval for a configured L2 account with a non-maker-only API key. Both testnets skip approval.
//!
//! During startup on either mainnet, when account details are available, the execution client
//! attempts to apply the `NAUTILUS` referral code to the account's public L1 address using the
//! configured L2 API key. The call is best effort with a 10-second timeout, and failures do not block
//! trading.
//! Selecting either mainnet opts the account into this attribution to support NautilusTrader
//! development.
//! Both testnets skip referral attribution.
//!
//! Lighter Mainnet's [referral program](https://docs.lighter.xyz/programs/referral-program) offers eligible
//! referred users a 10% trading fee discount, paid as rebates. The venue determines referral acceptance
//! and eligibility.
//!
//! See the [Lighter integration guide](https://nautilustrader.io/docs/nightly/integrations/lighter/#integrator-attribution)
//! for attribution and revocation details.

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
