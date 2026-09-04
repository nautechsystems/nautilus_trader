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

//! Test utilities and data management for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-testkit` crate provides testing utilities including test data management,
//! file handling, and common testing patterns. This crate supports robust testing workflows
//! across the entire NautilusTrader ecosystem with automated data downloads and validation:
//!
//! - **Test data management**: Automated downloading and caching of test datasets.
//! - **File utilities**: File integrity verification with SHA-256 checksums.
//! - **Path resolution**: Platform-agnostic test data path management.
//! - **Precision handling**: Support for both 64-bit and 128-bit precision test data.
//! - **Event collection**: Draining and correlating the data events a client emits.
//! - **Common patterns**: Reusable fixtures and test support.
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
//! This crate provides feature flags to control source code inclusion during compilation.
//!
//! - `datasets` (default): Enables test dataset discovery, download, validation, parsing, and
//!   loading.
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision`: Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `testers` (default): Enables test actors, strategies, and in-memory cache backing.
//!
//! Event collection utilities remain available without enabling a feature.

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
    clippy::assert_is_empty,
    reason = "`assert!(x.is_empty())` is clearer than comparing against an empty value"
)]
#![cfg_attr(
    test,
    allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::float_cmp,
        clippy::trivially_copy_pass_by_ref,
        reason = "test fixtures assert exact values and construct binary protocol bytes"
    )
)]

#[cfg(feature = "testers")]
pub mod cache;
#[cfg(feature = "datasets")]
pub mod common;
#[cfg(feature = "testers")]
pub mod components;
pub mod events;

#[cfg(feature = "datasets")]
pub mod files;
#[cfg(feature = "datasets")]
pub mod itch;

#[cfg(feature = "testers")]
pub mod testers;

// Re-export for convenience
#[cfg(feature = "testers")]
pub use testers::{DataTester, DataTesterConfig, ExecTester, ExecTesterConfig};

#[cfg(feature = "python")]
pub mod python;
