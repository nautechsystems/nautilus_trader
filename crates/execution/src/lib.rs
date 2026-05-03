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

//! Order execution system for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-execution` crate provides an order execution system that handles the complete
//! order lifecycle from submission to fill processing. This includes sophisticated order matching,
//! execution venue integration, and advanced order type emulation:
//!
//! - **Execution engine**: Central orchestration of order routing and position management.
//! - **Order matching engine**: High-fidelity market simulation for backtesting and paper trading.
//! - **Order emulator**: Advanced order types not natively supported by venues (trailing stops, contingent orders).
//! - **Execution clients**: Abstract interfaces for connecting to trading venues and brokers.
//! - **Order manager**: Local order lifecycle management and state tracking.
//! - **Matching core**: Low-level order book and price-time priority matching algorithms.
//! - **Fee and fill models**: Configurable execution cost simulation and realistic fill behavior.
//!
//! The crate supports both live trading environments (with real execution clients) and simulated
//! environments (with matching engines), making it suitable for production trading, strategy
//! development, and backtesting.
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
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `extension-module`: Builds the crate as a Python extension module.

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
    reason = "execution domain terms such as ts_event/ts_init are intentionally parallel"
)]
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "execution math casts between i64/u64/usize/f64 with values bounded by domain ranges"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "execution accessors and constructors are pervasive; #[must_use] noise is not warranted"
)]
#![allow(
    clippy::unused_self,
    reason = "engine and matching helpers take &self for method-style organization"
)]
#![allow(
    clippy::large_types_passed_by_value,
    reason = "command and report value types are intentionally moved through dispatch"
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
    reason = "config and snapshot structs mirror existing Python configuration surfaces"
)]
#![allow(
    clippy::too_many_lines,
    reason = "engine and matching dispatch functions exceed the default threshold by design"
)]
#![allow(
    clippy::inline_always,
    reason = "hot-path helpers in matching engine are intentionally always inlined"
)]
#![allow(
    clippy::match_same_arms,
    reason = "explicit per-variant arms document order/event dispatch even when bodies coincide"
)]
#![allow(
    clippy::match_wildcard_for_single_variants,
    reason = "wildcard arms guard against future enum variants in command dispatch"
)]
#![allow(
    clippy::manual_let_else,
    reason = "match-with-early-return is consistent with surrounding engine and reconciliation code"
)]
#![allow(
    clippy::single_match_else,
    reason = "two-arm matches are consistent with surrounding command and event dispatch"
)]
#![cfg_attr(
    test,
    allow(
        clippy::default_trait_access,
        clippy::float_cmp,
        clippy::should_panic_without_expect,
        clippy::unreadable_literal,
        clippy::used_underscore_binding,
        reason = "execution tests assert exact float outputs and use loose patterns for fixture setup"
    )
)]

pub mod client;
pub mod engine;
pub mod matching_core;
pub mod matching_engine;
pub mod models;
pub mod order_emulator;
pub mod order_manager;
pub mod protection;
pub mod reconciliation;
pub mod trailing;

#[cfg(feature = "python")]
pub mod python;
