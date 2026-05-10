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

//! Portfolio management and risk analysis for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-portfolio` crate provides portfolio management capabilities including
//! real-time position tracking, performance calculations, and risk management. This includes
//! sophisticated portfolio analytics and multi-currency support:
//!
//! - **Portfolio tracking**: Real-time portfolio state management with position and balance monitoring.
//! - **Account management**: Support for cash and margin accounts across multiple venues.
//! - **Performance calculations**: Real-time unrealized PnL, realized PnL, and mark-to-market valuations.
//! - **Risk management**: Initial margin calculations, maintenance margin tracking, and exposure monitoring.
//! - **Multi-currency support**: Currency conversion and cross-currency risk exposure analysis.
//! - **Configuration options**: Flexible settings for price types, currency conversion, and portfolio behavior.
//!
//! The crate handles complex portfolio scenarios including multi-venue trading, currency conversions,
//! and sophisticated margin calculations for both live trading and backtesting environments.
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
//! - `extension-module`: Builds the crate as a Python extension module.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod config;
pub mod manager;
pub mod portfolio;

#[cfg(feature = "python")]
pub mod python;

// Re-exports
pub use portfolio::Portfolio;
