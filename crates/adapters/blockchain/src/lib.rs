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

//! Blockchain data adapter for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-blockchain` crate loads DEX pools and token definitions and ingests real-time and
//! historical on-chain data for analytics and trading strategies. It connects RPC and optional
//! HyperSync sources to NautilusTrader for block, swap, and liquidity-event processing.
//!
//! Official API references: [Ethereum JSON-RPC](https://ethereum.org/en/developers/docs/apis/json-rpc/)
//! and [Envio HyperSync](https://docs.envio.dev/docs/HyperSync/overview).
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
//! - `extension-module`: Builds as a Python extension module.
//! - `hypersync`: Enables the
//!   [`hypersync-client`](https://crates.io/crates/hypersync-client) integration.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `turmoil`: Enables deterministic network simulation testing with
//!   [turmoil](https://crates.io/crates/turmoil).
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/blockchain/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/blockchain/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/blockchain)

#![warn(rustc::all)]
#![warn(clippy::clone_on_ref_ptr)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]

pub mod config;
pub mod constants;
pub mod contracts;
pub mod decode;
pub mod events;
pub mod math;
pub mod reporting;
pub mod rpc;

#[cfg(feature = "hypersync")]
pub mod cache;

#[cfg(feature = "hypersync")]
pub mod data;

#[cfg(feature = "hypersync")]
pub mod exchanges;

#[cfg(feature = "hypersync")]
pub mod execution;

#[cfg(feature = "hypersync")]
pub mod factories;

#[cfg(feature = "hypersync")]
pub mod hypersync;

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "hypersync")]
pub mod services;
