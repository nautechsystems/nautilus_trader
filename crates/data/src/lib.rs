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

//! Data engine and market data processing for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-data` crate provides a framework for handling market data ingestion,
//! processing, and aggregation within the NautilusTrader ecosystem. This includes real-time
//! data streaming, historical data management, and various aggregation methodologies:
//!
//! - High-performance data engine for orchestrating data operations.
//! - Data client infrastructure for connecting to market data providers.
//! - Bar aggregation machinery supporting tick, volume, value, and time-based aggregation.
//! - Order book management and delta processing capabilities.
//! - Subscription management and data request handling.
//! - Configurable data routing and processing pipelines.
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
//! - `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
//! - `high-precision`: Enables [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode) to use 128-bit value types.
//! - `streaming`: Enables `persistence` dependency for catalog-based data streaming.
//! - `defi`: Enables DeFi (Decentralized Finance) support.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod aggregation;
pub mod client;
pub mod engine;
pub mod option_chains;

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "defi")]
pub mod defi;

// Re-exports
pub use aggregation::{
    FixedTickSchemeRounder, MapVegaProvider, SpreadPriceRounder, SpreadQuoteAggregator,
    VegaProvider,
};
pub use client::DataClientAdapter;
