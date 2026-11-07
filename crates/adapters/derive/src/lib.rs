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
//! [Derive](https://www.derive.xyz).
//!
//! The `nautilus-derive` crate provides integration with the Derive self-custodial onchain
//! options, perpetuals, and spot exchange. Authentication uses the owner's EOA or multisig
//! together with a session-key signer; orders are EIP-712 typed-data
//! signed against the venue's per-action module contracts.
//!
//! The official Derive API reference can be found at <https://docs.derive.xyz/>.
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
//! - `simulation`: Enables deterministic simulation with `cfg(madsim)`.
//!
//! # Integrator Attribution
//!
//! Signed order, replace, and trigger-order payloads set `referral_code` to `nautilus`,
//! with no additional trading cost.
//!
//! See [attribution details](https://nautilustrader.io/docs/nightly/integrations/derive/#referral-code-attribution).
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/derive/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/derive/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/derive)

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
pub mod providers;
pub mod signing;
pub mod websocket;

mod book;

#[cfg(feature = "python")]
pub mod python;

pub use crate::{
    config::{DeriveDataClientConfig, DeriveExecutionClientConfig},
    execution::DeriveExecutionClient,
    factories::{DeriveDataClientFactory, DeriveExecutionClientFactory},
};

#[cfg(test)]
#[allow(
    dead_code,
    reason = "shared logging fixture also provides integration-only WebSocket servers"
)]
mod client_logging {
    include!("../../../network/tests/integration/common/client_logging.rs");
}
