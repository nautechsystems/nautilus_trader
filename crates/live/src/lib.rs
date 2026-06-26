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

//! Live system node for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-live` crate provides high-level abstractions and infrastructure for running live trading
//! systems, including data streaming, execution management, and system lifecycle handling.
//! It builds on top of the system kernel to provide simplified interfaces for live deployment:
//!
//! - `LiveNode` High-level abstraction for live system nodes.
//! - `LiveNodeConfig` Configuration for live node deployment.
//! - `AsyncRunner` for managing system real-time data flow.
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
//! - `node` (default): Enables the full live node, builder, config, and execution manager.
//! - `plugin` (default): Keeps compatibility stubs for plug-in config validation.
//! - `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
//! - `streaming`: Enables `persistence` dependency for streaming configuration (requires `node`).
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs) (auto-enables `node` and `streaming`).
//! - `defi`: Enables DeFi (Decentralized Finance) support.
//! - `extension-module`: Builds the crate as a Python extension module.
//!
//! # Lean adapter builds
//!
//! Adapters and other consumers that only need the async event emitter, runner, and
//! `ExecutionClientCore` re-export can opt out of the full kernel by disabling the
//! `node` feature:
//!
//! ```toml
//! nautilus-live = { workspace = true, default-features = false }
//! ```
//!
//! With `node` disabled, this crate exposes only `emitter` and `runner`, and skips
//! the transitive dependencies on `nautilus-system`, `nautilus-trading`,
//! `nautilus-portfolio`, `nautilus-risk`, and `nautilus-data`.
//!
//! # Plug-in support
//!
//! The open-source live crate does not host dynamic plug-ins directly.
//! `nautilus-plugin` is the public guest ABI crate, while host-side loading,
//! vtables, bridge adapters, and server policy belong to the host-side plug-in
//! integration.
//! A non-empty `LiveNodeConfig.plugins` list is rejected unless an application
//! provides that host-side integration.
//!
//! ```toml
//! nautilus-live = { workspace = true, default-features = false, features = ["node"] }
//! ```
//!
//! With `plugin` disabled, the compatibility `plugin` module is removed. A
//! non-empty `LiveNodeConfig.plugins` list is rejected under this configuration
//! as well.

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
    reason = "timing and plug-in identifiers such as elapsed_ms/elapsed_ns and loaded/loader are intentionally parallel"
)]
#![allow(
    clippy::single_match_else,
    reason = "match can be clearer than if-let-else for some reconciliation state transitions"
)]
#![allow(
    clippy::redundant_closure_for_method_calls,
    reason = "matches the Rust 1.94 ICE workaround in the workspace lint table"
)]
#![allow(
    clippy::too_many_lines,
    reason = "live node lifecycle and reconciliation flows exceed the default threshold by design"
)]
#![allow(
    clippy::unsafe_derive_deserialize,
    reason = "config types deserialize plain field values; unsafe in unrelated impls is sound"
)]

pub mod execution;
pub mod runner;

#[cfg(feature = "node")]
pub mod node;

#[cfg(feature = "python")]
pub mod python;

// Re-exports for adapters
pub use execution::{emitter, emitter::ExecutionEventEmitter, manager};
pub use nautilus_common::factories::OrderEventFactory;
pub use nautilus_execution::client::core::ExecutionClientCore;
#[cfg(feature = "plugin")]
pub use node::plugin;
#[cfg(feature = "node")]
pub use node::{builder, config};
