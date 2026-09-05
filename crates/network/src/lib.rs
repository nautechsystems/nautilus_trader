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

//! Network clients and connection policy for [NautilusTrader](https://nautilustrader.io).
//!
//! The crate provides asynchronous HTTP, reconnecting WebSocket, and suffix-framed TCP clients,
//! together with rate limiting, retry, backoff, proxy, and TLS support.
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
//! This crate provides feature flags to control source code inclusion during compilation:
//!
//! - `extension-module`: Builds as a Python extension module.
//! - `python`: Exposes the `TransportBackend` enum through [PyO3](https://pyo3.rs).
//! - `simulation`: Enables deterministic simulation testing with
//!   [MadSim](https://crates.io/crates/madsim).
//! - `transport-sockudo` (default): Adds the [sockudo-ws](https://crates.io/crates/sockudo-ws)
//!   WebSocket backend, selectable through `WebSocketConfig.backend`.
//! - `turmoil`: Enables deterministic network simulation testing with
//!   [turmoil](https://crates.io/crates/turmoil).
//!
//! # Testing
//!
//! The crate includes standard integration tests and deterministic failure-path tests using
//! `turmoil`.
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
//! The `turmoil` tests cover reconnections, partitions, and related network failures without
//! relying on wall-clock timing.

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
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "rate limiter and backoff arithmetic requires intentional narrowing casts"
)]
#![allow(
    clippy::too_many_lines,
    reason = "network client functions with connection management are complex by nature"
)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]
// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod backoff;
pub mod dst;
pub mod http;
pub mod mode;
pub mod net;
pub mod retry;
pub mod socket;
pub mod transport;
pub mod websocket;

mod heartbeat;
mod logging;
mod sink;
mod tls;

#[cfg(feature = "python")]
pub mod python;

pub mod error;
pub mod ratelimiter;

pub use sink::{SocketState, SocketStateSink};
pub use transport::{Message, TransportError};

/// Sentinel message indicating that a WebSocket reconnection completed.
pub const RECONNECTED: &str = "__RECONNECTED__";
