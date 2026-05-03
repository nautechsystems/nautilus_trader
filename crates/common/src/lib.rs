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

//! Common componentry for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-common` crate provides shared components and utilities that form the system foundation for
//! NautilusTrader applications. This includes the actor system, message bus, caching layer, and other
//! essential services.
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
//! - `defi`: Enables DeFi (Decentralized Finance) support.
//! - `indicators`: Includes the `nautilus-indicators` crate and indicator utilities.
//! - `capnp`: Enables [Cap'n Proto](https://capnproto.org/) serialization support.
//! - `live`: Enables the Tokio async runtime for live trading.
//! - `tracing-bridge`: Enables the `tracing` subscriber bridge for log integration.
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
    reason = "domain terms such as cache_greeks/cached_greeks and kv/k are intentionally parallel"
)]
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "common-layer math casts between i64/u64/usize/f64 with values bounded by domain ranges"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "common-layer accessors and constructors are pervasive; #[must_use] noise is not warranted"
)]
#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "trait method signatures are dictated by upstream interfaces and Python API parity"
)]
#![allow(
    clippy::unsafe_derive_deserialize,
    reason = "config types deserialize plain field values; unsafe in unrelated impls is sound"
)]
#![allow(
    clippy::missing_fields_in_debug,
    reason = "manual Debug impls intentionally omit verbose internal state and handler maps"
)]
#![allow(
    clippy::struct_excessive_bools,
    clippy::fn_params_excessive_bools,
    reason = "config structs and constructors mirror existing Python configuration surfaces"
)]
#![allow(
    clippy::too_many_lines,
    reason = "actor and message bus dispatch functions exceed the default threshold by design"
)]
#![allow(
    clippy::implicit_hasher,
    reason = "hash maps in public APIs intentionally accept the default hasher"
)]
#![allow(
    clippy::inline_always,
    reason = "hot-path helpers in throttler and clock are intentionally always inlined"
)]
#![allow(
    clippy::match_same_arms,
    reason = "explicit per-variant arms document message dispatch even when bodies coincide"
)]
#![cfg_attr(
    test,
    allow(
        clippy::default_trait_access,
        clippy::float_cmp,
        clippy::manual_let_else,
        clippy::no_effect_underscore_binding,
        clippy::should_panic_without_expect,
        clippy::single_match_else,
        clippy::unreadable_literal,
        clippy::unused_self,
        clippy::used_underscore_binding,
        reason = "common tests assert exact float outputs and use loose patterns for fixture setup"
    )
)]

pub mod actor;
pub mod cache;
pub mod clients;
pub mod clock;
pub mod component;
pub mod custom;
pub mod enums;
pub mod factories;
pub mod generators;
pub mod greeks;
pub mod logging;
mod macros;
pub mod messages;
pub mod msgbus;
pub mod providers;
pub mod runner;
pub mod signal;
pub mod testing;
pub mod throttler;
pub mod timer;
pub mod xrate;

#[cfg(feature = "live")]
pub mod live;

#[cfg(feature = "defi")]
pub mod defi;

#[cfg(feature = "ffi")]
pub mod ffi;

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "capnp")]
pub mod serialization;
