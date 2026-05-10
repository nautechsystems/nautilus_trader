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
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

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
