// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! [NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
//! algorithmic trading platform, providing quantitative traders with the ability to backtest
//! portfolios of automated trading strategies on historical data with an event-driven engine,
//! and also deploy those same strategies live, with no code changes.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `ffi`: Enables the C foreign function interface (FFI) from [cbindgen](https://github.com/mozilla/cbindgen).
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
//! - `stubs`: Enables type stubs for use in testing scenarios.
#![warn(rustc::all)]
#![deny(nonstandard_style)]
#![deny(rustdoc::broken_intra_doc_links)]
// #![deny(clippy::missing_errors_doc)]

// Uncomment once we've added trivial `Debug` impls everywhere
// #![warn(missing_debug_implementations)]

pub mod actor;
pub mod cache;
pub mod clock;
pub mod component;
pub mod custom;
pub mod enums;
pub mod factories;
pub mod generators;
pub mod logging;
pub mod messages;
pub mod msgbus;
pub mod runner;
pub mod runtime;
pub mod signal;
pub mod testing;
pub mod throttler;
pub mod timer;
pub mod xrate;

#[cfg(feature = "ffi")]
pub mod ffi;

#[cfg(feature = "python")]
pub mod python;
