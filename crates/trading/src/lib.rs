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

//! Trading strategy machinery and orchestration [NautilusTrader](http://nautilustrader.io).
//!
//! The *trading* crate provides core trading orchestration capabilities including component
//! lifecycle management, strategy coordination, and market session utilities. This crate serves
//! as the central coordination layer between trading strategies, execution systems, and market data:
//!
//! - **Trader orchestration**: Central coordination of trading components and lifecycle management.
//! - **Component management**: Registration, state tracking, and control of actors and strategies.
//! - **Forex sessions**: Market session time calculations and timezone handling.
//! - **Strategy coordination**: Integration layer for algorithmic trading strategies.
//! - **System integration**: Bridge between kernel and trading component subsystems.
//!
//! # Feature flags
//!
//! This crate provides feature flags to control source code inclusion during compilation,
//! depending on the intended use case, i.e. whether to provide Python bindings
//! for the [nautilus_trader](https://pypi.org/project/nautilus_trader) Python package,
//! or as part of a Rust only build.
//!
//! - `python`: Enables Python bindings from [PyO3](https://pyo3.rs).

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod sessions;
pub mod trader;

#[cfg(feature = "python")]
pub mod python;
