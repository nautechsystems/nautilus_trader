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

//! Network functionality for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-network` crate provides networking components including HTTP, WebSocket, and raw TCP socket
//! clients, rate limiting, backoff strategies, and socket TLS utilities for connecting to
//! trading venues and data providers.
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
//! - `turmoil`: Enables deterministic network simulation testing with [turmoil](https://github.com/tokio-rs/turmoil).
//! - `transport-sockudo`: Adds the [sockudo-ws](https://crates.io/crates/sockudo-ws) WebSocket backend, selectable via `WebSocketConfig.backend`.
//!
//! # Testing
//!
//! The crate includes both standard integration tests and deterministic network simulation tests using turmoil.
//!
//! To run standard tests:
//! ```bash
//! cargo nextest run -p nautilus-network
//! ```
//!
//! To run turmoil network simulation tests:
//! ```bash
//! cargo nextest run -p nautilus-network --features turmoil
//! ```
//!
//! The turmoil tests simulate various network conditions (reconnections, partitions, etc.) in a deterministic way,
//! allowing reliable testing of network failure scenarios without flakiness.

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
    clippy::inline_always,
    reason = "hot-path functions use #[inline(always)] intentionally for constant-folding"
)]
#![allow(
    clippy::manual_let_else,
    reason = "match can be clearer than let-else for some patterns"
)]
#![allow(
    clippy::redundant_closure_for_method_calls,
    reason = "causes clippy ICE on Rust 1.94; matches the workaround in workspace Cargo.toml"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "rate limiter and backoff arithmetic requires intentional narrowing casts"
)]
#![allow(
    clippy::too_many_lines,
    reason = "network client functions with connection management are complex by nature"
)]

pub mod backoff;
pub mod dst;
pub mod http;
pub mod mode;
pub mod net;
pub mod retry;
pub mod socket;
pub mod transport;
pub mod websocket;

mod logging;
mod tls;

#[cfg(feature = "python")]
pub mod python;

pub mod error;
pub mod ratelimiter;

pub use transport::{Message, TransportError};

/// Sentinel message to signal reconnection completion to Rust consumers.
pub const RECONNECTED: &str = "__RECONNECTED__";
