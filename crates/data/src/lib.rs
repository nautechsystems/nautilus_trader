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
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::similar_names,
    reason = "data domain terms such as ts_event/ts_init are intentionally parallel"
)]
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "data math casts between i64/u64/usize/f64 with values bounded by domain ranges"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "data accessors are pervasive; #[must_use] noise is not warranted"
)]
#![allow(
    clippy::unused_self,
    reason = "engine helpers take &self for method-style organization"
)]
#![allow(
    clippy::large_types_passed_by_value,
    reason = "command and request value types are intentionally moved through dispatch"
)]
#![allow(
    clippy::unsafe_derive_deserialize,
    reason = "config types deserialize plain field values; unrelated unsafe impls are sound"
)]
#![allow(
    clippy::missing_fields_in_debug,
    reason = "manual Debug impls intentionally omit verbose internal state"
)]
#![allow(
    clippy::struct_excessive_bools,
    reason = "config structs mirror existing Python configuration surfaces"
)]
#![allow(
    clippy::too_many_lines,
    reason = "engine and aggregation dispatch functions exceed the default threshold by design"
)]
#![allow(
    clippy::inline_always,
    reason = "hot-path helpers in aggregation are intentionally always inlined"
)]
#![allow(
    clippy::match_same_arms,
    reason = "explicit per-variant arms document data dispatch even when bodies coincide"
)]
#![allow(
    clippy::match_wildcard_for_single_variants,
    reason = "wildcard arms guard against future enum variants in command dispatch"
)]
#![allow(
    clippy::single_match_else,
    reason = "two-arm matches are consistent with surrounding command and event dispatch"
)]
#![cfg_attr(
    test,
    allow(
        clippy::float_cmp,
        clippy::should_panic_without_expect,
        clippy::unreadable_literal,
        clippy::used_underscore_binding,
        reason = "data tests assert exact float outputs and use loose patterns for fixture setup"
    )
)]

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
