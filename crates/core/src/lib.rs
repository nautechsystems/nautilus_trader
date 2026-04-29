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

//! Core foundational types and utilities for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-core` crate is designed to be lightweight, efficient, and to provide zero-cost abstractions
//! wherever possible. It supplies the essential building blocks used across the NautilusTrader
//! ecosystem, including:
//!
//! - Time handling and atomic clock functionality.
//! - UUID generation and management.
//! - Mathematical functions and interpolation utilities.
//! - Correctness validation functions.
//! - Serialization traits and helpers.
//! - Cross-platform environment utilities.
//! - Abstractions over common collections.
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
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::inline_always,
    reason = "hot-path predicate guards use #[inline(always)] intentionally for constant-folding"
)]
#![allow(
    clippy::manual_let_else,
    reason = "match can be clearer than let-else for some patterns"
)]
#![allow(
    clippy::redundant_closure_for_method_calls,
    reason = "causes clippy ICE on Rust 1.94; matches the workaround in workspace Cargo.toml"
)]

pub mod collections;
pub mod consts;
pub mod correctness;
pub mod datetime;
pub mod drop;
pub mod env;
pub mod hex;
pub mod math;
pub mod message;
pub mod nanos;
pub mod params;
pub mod paths;
pub mod serialization;
pub mod shared;
pub mod string;
pub mod time;
pub mod uuid;

#[cfg(feature = "ffi")]
pub mod ffi;

#[cfg(feature = "python")]
pub mod python;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("Unsupported platform: Nautilus supports only Linux, macOS, and Windows");

// Re-exports
#[cfg(feature = "python")]
pub use crate::params::from_pydict;
pub use crate::{
    collections::{AtomicMap, AtomicSet},
    drop::CleanDrop,
    nanos::UnixNanos,
    params::Params,
    shared::{SharedCell, WeakCell},
    string::stack_str::{STACKSTR_CAPACITY, StackStr},
    time::AtomicTime,
    uuid::UUID4,
};

/// Message for when a mutex guard cannot be acquired due to poisoning.
///
/// Mutex guards should use `expect` rather than handle poison errors.
/// A poisoned mutex indicates a thread panicked while holding the lock,
/// meaning protected data may be in an inconsistent state. Propagating
/// the panic is the idiomatic and safe approach, as continuing with
/// potentially corrupted data would violate safety invariants.
pub const MUTEX_POISONED: &str = "Mutex poisoned";
