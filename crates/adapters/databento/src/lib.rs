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

//! [NautilusTrader](https://nautilustrader.io) adapter for [Databento](https://databento.com).
//!
//! The `nautilus-databento` crate provides live and historical market data across venues and asset classes
//! through Databento's official Rust client. It loads Databento Binary Encoding (DBN) files, decodes
//! market events and instrument definitions into Nautilus types, and supports live-node integration
//! and optional Python bindings. It provides no order execution.
//!
//! The official Databento API reference can be found at <https://databento.com/docs>.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native trading engine
//! with a deterministic, event-driven architecture for multi-asset, multi-venue systems.
//!
//! The system spans research, simulation, and live execution, providing research-to-live semantic parity.
//!
//! # Feature Flags
//!
//! Feature flags control code inclusion for Rust builds and Python bindings:
//!
//! - `arrow`: Enables Apache Arrow data support.
//! - `extension-module`: Builds as a Python extension module.
//! - `high-precision` (default): Enables
//!   [high-precision mode](https://nautilustrader.io/docs/nightly/getting_started/installation/#precision-mode)
//!   to use 128-bit value types.
//! - `live` (default): Enables live data functionality, including the `data`, `factories`, and
//!   `live` modules.
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//!
//! # Documentation
//!
//! - [Integration guide](https://nautilustrader.io/docs/nightly/integrations/databento/)
//! - [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/databento/examples)
//! - [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/databento)

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]
// pyo3's `from_py_object` generates `.clone()` on `Copy` fields that clippy flags from the
// macro expansion; an item-level `allow` cannot reach the expansion
#![allow(clippy::clone_on_copy)]

pub mod common;
pub mod decode;
pub mod enums;
pub mod historical;
pub mod loader;
pub mod symbology;
pub mod types;

#[cfg(feature = "arrow")]
pub mod arrow;

#[cfg(feature = "live")]
pub mod data;

#[cfg(feature = "live")]
pub mod factories;

#[cfg(feature = "live")]
pub mod live;

#[cfg(feature = "python")]
pub mod python;
