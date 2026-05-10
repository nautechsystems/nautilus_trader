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

//! Trading strategy machinery and orchestration [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-trading` crate provides core trading capabilities including:
//!
//! - **Forex sessions**: Market session time calculations and timezone handling.
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
//! - `examples`: Enables example strategies (e.g. `EmaCross`) for backtesting and demos.
//! - `defi`: Enables DeFi (Decentralized Finance) support.
//! - `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds the crate as a Python extension module.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

mod macros;

#[doc(hidden)]
pub mod _macro_reexports {
    pub use nautilus_common::actor::DataActorCore;
}

pub mod algorithm;
pub mod sessions;
pub mod strategy;

#[cfg(feature = "examples")]
pub mod examples;

pub use algorithm::{
    ExecutionAlgorithm, ExecutionAlgorithmConfig, ExecutionAlgorithmCore,
    ImportableExecAlgorithmConfig, TwapAlgorithm, TwapAlgorithmConfig,
};
pub use strategy::{ImportableStrategyConfig, Strategy, StrategyConfig, StrategyCore};

#[cfg(feature = "python")]
pub mod python;
