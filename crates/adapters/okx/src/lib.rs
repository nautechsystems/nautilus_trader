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

//! [NautilusTrader](https://nautilustrader.io) adapter for the [OKX](https://www.okx.com) cryptocurrency exchange.
//!
//! The `nautilus-okx` crate provides client bindings (HTTP & WebSocket) and data
//! models for the official **OKX v5 API**.
//!
//! The official OKX API reference can be found at <https://www.okx.com/docs-v5/en/>.
//! All public links inside this crate reference the English version.
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
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `simulation`: Enables deterministic simulation testing with
//!   [MadSim](https://crates.io/crates/madsim).

#![warn(rustc::all)]
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::similar_names,
    reason = "venue and domain terms such as inst/inst_id and bid/ask are intentionally parallel"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "venue protocol casts between i64/u64/usize with values bounded by documented OKX ranges"
)]
#![allow(
    clippy::too_many_lines,
    reason = "venue message parsing and request dispatch functions with large match statements are complex by nature"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "client accessors and constructors are pervasive; #[must_use] noise is not warranted"
)]
#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "PyO3 methods and trait method signatures require pass-by-reference parity"
)]
#![allow(
    clippy::unsafe_derive_deserialize,
    reason = "config and message types deserialize plain field values; unsafe in unrelated impls is sound"
)]
#![allow(
    clippy::unused_self,
    reason = "PyO3 methods and client trait operations take &self for interface parity"
)]
#![allow(
    clippy::match_same_arms,
    reason = "explicit per-variant arms document venue enum mappings even when bodies coincide"
)]
#![allow(
    clippy::match_wildcard_for_single_variants,
    reason = "wildcard arms guard against future enum variants in venue message dispatch"
)]
#![allow(
    clippy::manual_let_else,
    reason = "match can be clearer than let-else for some patterns"
)]
#![allow(
    clippy::single_match_else,
    reason = "match can be clearer than if-let-else for some patterns"
)]
#![allow(
    clippy::redundant_else,
    reason = "sometimes explicit else blocks improve readability"
)]
// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod common;
pub mod config;
pub mod data;
pub mod execution;
pub mod factories;
pub mod http;
pub mod websocket;

mod book_sync;

#[cfg(feature = "python")]
pub mod python;

// Re-exports
pub use crate::{
    common::{
        enums::{OKXInstrumentType, OKXOrderType, OKXPositionMode, OKXPositionSide, OKXSide},
        models::OKXInstrument,
    },
    data::OKXDataClient,
    execution::OKXExecutionClient,
    http::{client::OKXHttpClient, error::OKXHttpError},
    websocket::{client::OKXWebSocketClient, error::OKXWsError},
};
