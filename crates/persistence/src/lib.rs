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

//! Data persistence and storage management for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-persistence` crate provides data persistence capabilities for storing and retrieving
//! trading data, state, and configuration.
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
//! This crate provides the following feature flags:
//!
//! - `cloud`: Enables the `cloud` feature.
//! - `defi`: Enables decentralized finance support.
//! - `extension-module`: Builds the Python extension module.
//! - `high-precision`: Enables 128-bit fixed-point value types.
//! - `python`: Enables Python bindings through `PyO3`.

#![warn(rustc::all)]
#![warn(clippy::pedantic)]
#![deny(nonstandard_style)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(rustdoc::broken_intra_doc_links)]
// #![deny(clippy::missing_errors_doc)]
#![allow(
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]
// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod backend;
pub mod catalog;
pub mod common;
pub mod writer;

#[cfg(feature = "python")]
pub mod python;
